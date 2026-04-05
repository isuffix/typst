use ecow::eco_format;
use typst_library::diag::{At, Hint, SourceResult, Trace, Tracepoint, bail, error};
use typst_library::foundations::{Args, Dict, Str, Value};
use typst_syntax::Span;
use typst_syntax::ast::{self, AstNode};

use crate::{Eval, Vm};

/// Access an expression mutably.
pub(crate) trait Access {
    /// Access the expression's evaluated value mutably.
    fn access<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value>;
}

impl Access for ast::Expr<'_> {
    fn access<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        match self {
            Self::Ident(v) => v.access(vm),
            Self::Parenthesized(v) => v.access(vm),
            Self::FieldAccess(v) => v.access(vm),
            Self::FuncCall(v) => v.access(vm),
            _ => {
                let _ = self.eval(vm)?;
                bail!(self.span(), "cannot mutate a temporary value");
            }
        }
    }
}

impl Access for ast::Ident<'_> {
    fn access<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        let span = self.span();
        if vm.inspected == Some(span)
            && let Ok(binding) = vm.scopes.get(&self)
        {
            vm.trace(binding.read().clone());
        }
        vm.scopes
            .get_mut(&self)
            .and_then(|b| b.write().map_err(Into::into))
            .at(span)
    }
}

impl Access for ast::Parenthesized<'_> {
    fn access<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        self.expr().access(vm)
    }
}

impl Access for ast::FieldAccess<'_> {
    fn access<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        access_dict(vm, self)?.at_mut(self.field().get()).at(self.span())
    }
}

pub(crate) fn access_dict<'a>(
    vm: &'a mut Vm,
    access: ast::FieldAccess,
) -> SourceResult<&'a mut Dict> {
    match access.target().access(vm)? {
        Value::Dict(dict) => Ok(dict),
        value => {
            let ty = value.ty();
            let span = access.target().span();
            if matches!(
                value, // those types have their own field getters
                Value::Symbol(_) | Value::Content(_) | Value::Module(_) | Value::Func(_)
            ) {
                bail!(span, "cannot mutate fields on {ty}");
            } else if typst_library::foundations::fields_on(ty).is_empty() {
                bail!(span, "{ty} does not have accessible fields");
            } else {
                // type supports static fields, which don't yet have
                // setters
                Err(eco_format!("fields on {ty} are not yet mutable"))
                    .hint(eco_format!(
                        "try creating a new {ty} with the updated field value instead"
                    ))
                    .at(span)
            }
        }
    }
}

impl Access for ast::FuncCall<'_> {
    fn access<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        if let ast::Expr::FieldAccess(access) = self.callee()
            && let method = access.field()
            && is_accessor_method(&method)
        {
            let span = self.span();
            let world = vm.world();
            let args = self.args().eval(vm)?.spanned(span);
            let value = access.target().access(vm)?;
            let result = call_method_access(value, &method, args, span);
            let point = || Tracepoint::Call(Some(method.get().clone()));
            result.trace(world, point, span)
        } else {
            let _ = self.eval(vm)?;
            bail!(self.span(), "cannot mutate a temporary value");
        }
    }
}

/// Whether a specific method is an accessor.
fn is_accessor_method(method: &str) -> bool {
    matches!(method, "first" | "last" | "at")
}

/// Call an accessor method on a value.
fn call_method_access<'a>(
    value: &'a mut Value,
    method: &str,
    mut args: Args,
    span: Span,
) -> SourceResult<&'a mut Value> {
    let ty = value.ty();
    let missing = || error!(span, "type {ty} has no method `{method}`");

    let slot = match value {
        Value::Array(array) => match method {
            "first" => array.first_mut().at(span)?,
            "last" => array.last_mut().at(span)?,
            "at" => array.at_mut(args.expect("index")?).at(span)?,
            _ => bail!(missing()),
        },
        Value::Dict(dict) => match method {
            "at" => dict.at_mut(&args.expect::<Str>("key")?).at(span)?,
            _ => bail!(missing()),
        },
        _ => bail!(missing()),
    };

    args.finish()?;
    Ok(slot)
}

/// Whether a specific method is mutating.
pub(crate) fn is_mutating_method(method: &str) -> bool {
    matches!(method, "push" | "pop" | "insert" | "remove")
}

/// Mutating methods for dictionaries.
fn is_dict_mutating_method(method: &str) -> bool {
    matches!(method, "insert" | "remove")
}

/// Attempt to resolve a mutating method call by evaluating args and then
/// attempting to access the target mutably. If the target's type doesn't
/// support mutating methods (only Array/Dict actually do), returns the
/// evaluated value and arguments.
///
/// This currently causes a number of bad errors due to limitations of the
/// [`Access`] trait used for mutation.
pub(crate) fn maybe_resolve_mutating(
    vm: &mut Vm,
    target: ast::Expr,
    field: ast::Ident,
    args: ast::Args,
    span: Span,
) -> SourceResult<Result<Value, (Value, Args)>> {
    // We evaluate the arguments first because `target_expr.access(vm)` mutably
    // borrows `vm`, so we won't be able to call `args.eval(vm)` afterwards.
    let args = args.eval(vm)?.spanned(span);
    match target.access(vm)? {
        // Skip methods that aren't actually mutating for dictionaries.
        target @ Value::Dict(_) if !is_dict_mutating_method(field.as_str()) => {
            Ok(Err((target.clone(), args)))
        }
        // Only arrays and dictionaries have mutable methods.
        target @ (Value::Array(_) | Value::Dict(_)) => {
            let value = call_method_mut(target, &field, args, span);
            let point = || Tracepoint::Call(Some(field.get().clone()));
            Ok(Ok(value.trace(vm.world(), point, span)?))
        }
        target => Ok(Err((target.clone(), args))),
    }
}

/// Call a mutating method on a value.
fn call_method_mut(
    value: &mut Value,
    method: &str,
    mut args: Args,
    span: Span,
) -> SourceResult<Value> {
    let ty = value.ty();
    let missing = || error!(span, "type {ty} has no method `{method}`");
    let mut output = Value::None;

    match value {
        Value::Array(array) => match method {
            "push" => array.push(args.expect("value")?),
            "pop" => output = array.pop().at(span)?,
            "insert" => {
                array.insert(args.expect("index")?, args.expect("value")?).at(span)?
            }
            "remove" => {
                output = array
                    .remove(args.expect("index")?, args.named("default")?)
                    .at(span)?
            }
            _ => bail!(missing()),
        },

        Value::Dict(dict) => match method {
            "insert" => dict.insert(args.expect::<Str>("key")?, args.expect("value")?),
            "remove" => {
                output =
                    dict.remove(args.expect("key")?, args.named("default")?).at(span)?
            }
            _ => bail!(missing()),
        },

        _ => bail!(missing()),
    }

    args.finish()?;
    Ok(output)
}

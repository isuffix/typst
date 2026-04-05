use ecow::{EcoString, eco_format};
use typst_library::diag::{At, Hint, SourceResult, Trace, Tracepoint, bail};
use typst_library::foundations::{Args, Dict, Str, Value};
use typst_syntax::Span;
use typst_syntax::ast::{self, AstNode};

use crate::call::{FieldCallee, call_func, eval_field_callee};
use crate::{Eval, Vm};

/// Access an expression mutably.
pub(crate) trait Access {
    /// Access the expression's evaluated value mutably.
    fn access_direct<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value>;

    fn access_indirect_or_eval(
        self,
        vm: &mut Vm,
    ) -> SourceResult<Result<IndirectAccess, Value>>;
}

pub(crate) struct IndirectAccess {
    /// The final value that should be accessed when the pattern is replayed.
    ///
    /// This may not actually be the final accessed value if the pattern is
    /// replayed if the base identifier (or anything in the access pattern) is
    /// mutated. Assuming otherwise could lead to a classic time-of-check vs.
    /// time-of-use (TOCTOU) error.
    ///
    /// We could automatically error for that if we wanted to by storing a
    /// unique value per mutable access location in the identifier's Binding.
    /// However, the obvious choice of a `Span` would not be fit for this task
    /// as they are overriden for strings passed to the `eval` function.
    pub value: Value,
    /// The base identifier of the pattern.
    base: EcoString,
    /// The access pattern.
    pattern: Vec<PatternPart>,
}

/// Part of the pattern for a mutable access and the span for potential errors.
enum PatternPart {
    Key(EcoString, Span),
    Index(i64, Span),
}

impl IndirectAccess {
    /// Replay the indirect access and get the mutable value.
    pub fn access<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        let Self { value, base, pattern } = self;
        // Avoid an extra copy-on-write of the base value when calling `at_mut`.
        drop(value);
        let mut current = vm.scopes.get_mut(&base).unwrap().write().unwrap();
        for part in pattern {
            match (current, part) {
                (Value::Dict(dict), PatternPart::Key(key, span)) => {
                    current = dict.at_mut(&key).at(span)?;
                }
                (Value::Array(array), PatternPart::Index(index, span)) => {
                    current = array.at_mut(index).at(span)?;
                }
                (_, PatternPart::Key(_, span)) => {
                    bail!(span, "this is no longer a dict!")
                }
                (_, PatternPart::Index(_, span)) => {
                    bail!(span, "this is no longer an array!")
                }
            }
        }
        Ok(current)
    }
}

impl Access for ast::Expr<'_> {
    fn access_direct<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        match self {
            Self::Ident(v) => v.access_direct(vm),
            Self::Parenthesized(v) => v.access_direct(vm),
            Self::FieldAccess(v) => v.access_direct(vm),
            Self::FuncCall(v) => v.access_direct(vm),
            _ => {
                let _ = self.eval(vm)?;
                bail!(self.span(), "cannot mutate a temporary value");
            }
        }
    }

    fn access_indirect_or_eval(
        self,
        vm: &mut Vm,
    ) -> SourceResult<Result<IndirectAccess, Value>> {
        match self {
            Self::Ident(v) => v.access_indirect_or_eval(vm),
            Self::MathIdent(v) => bail!(v.span(), "cannot mutate variables in math"),
            Self::Parenthesized(v) => v.access_indirect_or_eval(vm),
            Self::FieldAccess(v) => v.access_indirect_or_eval(vm),
            Self::FuncCall(v) => v.access_indirect_or_eval(vm),
            _ => Ok(Err(self.eval(vm)?)),
        }
    }
}

impl Access for ast::Ident<'_> {
    fn access_direct<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
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

    fn access_indirect_or_eval(
        self,
        vm: &mut Vm,
    ) -> SourceResult<Result<IndirectAccess, Value>> {
        let span = self.span();
        let value = vm.scopes.get(&self).at(span)?.read().clone();
        if vm.inspected == Some(span) {
            vm.trace(value.clone());
        }
        if let Ok(binding_mut) = vm.scopes.get_mut(&self)
            && binding_mut.write().is_ok()
        {
            Ok(Ok(IndirectAccess {
                base: self.get().clone(),
                value,
                pattern: Vec::new(),
            }))
        } else {
            Ok(Err(value))
        }
    }
}

impl Access for ast::Parenthesized<'_> {
    fn access_direct<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        self.expr().access_direct(vm)
    }

    fn access_indirect_or_eval(
        self,
        vm: &mut Vm,
    ) -> SourceResult<Result<IndirectAccess, Value>> {
        self.expr().access_indirect_or_eval(vm)
    }
}

impl Access for ast::FieldAccess<'_> {
    fn access_direct<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        access_dict(vm, self)?.at_mut(self.field().get()).at(self.span())
    }

    fn access_indirect_or_eval(
        self,
        vm: &mut Vm,
    ) -> SourceResult<Result<IndirectAccess, Value>> {
        let field = self.field();
        match self.target().access_indirect_or_eval(vm)? {
            Ok(IndirectAccess { value: Value::Dict(dict), base, mut pattern }) => {
                let key = field.get().clone();
                pattern.push(PatternPart::Key(key, field.span()));
                let value = dict.get(&field).at(self.span())?.clone();
                Ok(Ok(IndirectAccess { value, base, pattern }))
            }
            Err(target) | Ok(IndirectAccess { value: target, .. }) => {
                let value =
                    crate::code::access_field(vm, target, field.as_str(), field.span())?;
                Ok(Err(value))
            }
        }
    }
}

pub(crate) fn access_dict<'a>(
    vm: &'a mut Vm,
    access: ast::FieldAccess,
) -> SourceResult<&'a mut Dict> {
    match access.target().access_direct(vm)? {
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
    fn access_direct<'a>(self, vm: &'a mut Vm) -> SourceResult<&'a mut Value> {
        if let ast::Expr::FieldAccess(access) = self.callee()
            && let method = access.field()
            && maybe_accessor_method(&method)
        {
            let span = self.span();
            let world = vm.world();
            let args = self.args().eval(vm)?.spanned(span);
            let value = access.target().access_direct(vm)?;
            let result = call_accessor_method(value, &method, args, span);
            let point = || Tracepoint::Call(Some(method.get().clone()));
            result.trace(world, point, span)
        } else {
            let _ = self.eval(vm)?;
            bail!(self.span(), "cannot mutate a temporary value");
        }
    }

    fn access_indirect_or_eval(
        self,
        vm: &mut Vm,
    ) -> SourceResult<Result<IndirectAccess, Value>> {
        let span = self.span();
        let callee = self.callee();
        if let ast::Expr::FieldAccess(access) = callee
            && let method = access.field()
            && maybe_accessor_method(&method)
        {
            let target_expr = access.target();
            match target_expr.access_indirect_or_eval(vm)? {
                Ok(mut indirect) if is_accessor_method(&indirect.value, &method) => {
                    let method_name = method.get();
                    let mut args = self.args().eval(vm)?.spanned(self.span());
                    let (result, pattern_part) = match indirect.value {
                        Value::Array(array) if method_name == "first" => {
                            (array.at(0, None), PatternPart::Index(0, span))
                        }
                        Value::Array(array) if method_name == "last" => {
                            (array.at(-1, None), PatternPart::Index(-1, span))
                        }
                        Value::Array(array) if method_name == "at" => {
                            let i = args.expect("index")?;
                            (array.at(i, None), PatternPart::Index(i, span))
                        }
                        Value::Dict(dict) if method_name == "at" => {
                            let k: Str = args.expect("key")?;
                            (dict.at(k.clone(), None), PatternPart::Key(k.into(), span))
                        }
                        _ => unreachable!(),
                    };
                    args.finish()?;
                    let point = || Tracepoint::Call(Some(method_name.clone()));
                    indirect.value = result.at(span).trace(vm.world(), point, span)?;
                    indirect.pattern.push(pattern_part);
                    Ok(Ok(indirect))
                }
                Err(target) | Ok(IndirectAccess { value: target, .. }) => {
                    let mut args = self.args().eval(vm)?.spanned(span);
                    match eval_field_callee(
                        vm,
                        access.to_untyped(),
                        method.as_str(),
                        method.span(),
                        target,
                        false,
                    )? {
                        FieldCallee::Func(func) => {
                            Ok(Err(call_func(vm, func, args, span)?))
                        }
                        FieldCallee::Method(func, target) => {
                            // Method calls pass the target as the first argument.
                            args.insert(0, target_expr.span(), target);
                            Ok(Err(call_func(vm, func, args, span)?))
                        }
                        FieldCallee::NonFunc(_, err) => Err(err).at(callee.span()),
                    }
                }
            }
        } else {
            let value = self.eval(vm)?;
            Ok(Err(value))
        }
    }
}

/// Whether a method might be an accessor. May be a false-positive.
fn maybe_accessor_method(method: &str) -> bool {
    matches!(method, "first" | "last" | "at")
}

/// Whether a method is an accessor for the given value.
fn is_accessor_method(value: &Value, method: &str) -> bool {
    matches!(
        (value, method),
        (Value::Array(_), "first" | "last" | "at") | (Value::Dict(_), "at")
    )
}

/// Call an accessor method on a value.
fn call_accessor_method<'a>(
    value: &'a mut Value,
    method: &str,
    mut args: Args,
    span: Span,
) -> SourceResult<&'a mut Value> {
    if !is_accessor_method(value, method) {
        let ty = value.ty();
        // TODO: Add tests for a symbol with a `.at()` method.
        bail!(span, "type {ty} has no method `{method}`");
    }

    let slot = match value {
        Value::Array(array) => match method {
            "first" => array.first_mut().at(span)?,
            "last" => array.last_mut().at(span)?,
            "at" => array.at_mut(args.expect("index")?).at(span)?,
            _ => unreachable!(),
        },
        Value::Dict(dict) => match method {
            "at" => dict.at_mut(&args.expect::<Str>("key")?).at(span)?,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };

    args.finish()?;
    Ok(slot)
}

/// Whether a method might be mutating. May be a false-positive.
pub(crate) fn maybe_mutating_method(method: &str) -> bool {
    matches!(method, "push" | "pop" | "insert" | "remove")
}

/// Whether a method is mutating for the given value.
pub(crate) fn is_mutating_method(value: &Value, method: &str) -> bool {
    matches!(
        (value, method),
        (Value::Array(_), "push" | "pop" | "insert" | "remove")
            | (Value::Dict(_), "insert" | "remove")
    )
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
) -> SourceResult<Result<Value, Value>> {
    match target.access_indirect_or_eval(vm)? {
        Ok(indirect) if is_mutating_method(&indirect.value, &field) => {
            let args = args.eval(vm)?.spanned(span);
            let value = indirect.access(vm)?;
            let result = call_mutating_method(value, &field, args, span);
            let point = || Tracepoint::Call(Some(field.get().clone()));
            let resolved = result.trace(vm.world(), point, span)?;
            Ok(Ok(resolved))
        }
        Err(value) | Ok(IndirectAccess { value, .. }) => Ok(Err(value)),
    }
}

/// Call a mutating method on a value.
fn call_mutating_method(
    value: &mut Value,
    method: &str,
    mut args: Args,
    span: Span,
) -> SourceResult<Value> {
    if !is_mutating_method(value, method) {
        bail!(span, "this value has been modified!")
    }

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
            _ => unreachable!(),
        },
        Value::Dict(dict) => match method {
            "insert" => dict.insert(args.expect::<Str>("key")?, args.expect("value")?),
            "remove" => {
                output =
                    dict.remove(args.expect("key")?, args.named("default")?).at(span)?
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }

    args.finish()?;
    Ok(output)
}

use ecow::{EcoString, eco_format, eco_vec};
use typst_library::diag::{SourceDiagnostic, SourceResult, bail};
use typst_library::foundations::{
    Content, Func, NativeElement, Symbol, SymbolElem, Value,
};
use typst_library::math::{
    AlignPointElem, AttachElem, EquationElem, FracElem, LrElem, PrimesElem, RootElem,
};
use typst_library::text::TextElem;
use typst_syntax::ast::{self, AstNode, MathAccess, MathTextKind};

use crate::{Eval, Vm};

impl Eval for ast::Equation<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let body = self.body().eval(vm)?;
        let block = self.block();
        Ok(EquationElem::new(body).with_block(block).pack())
    }
}

impl Eval for ast::Math<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        Ok(Content::sequence(
            self.exprs()
                .map(|expr| expr.eval_display(vm))
                .collect::<SourceResult<Vec<_>>>()?,
        ))
    }
}

impl Eval for ast::MathText<'_> {
    type Output = Content;

    fn eval(self, _: &mut Vm) -> SourceResult<Self::Output> {
        match self.get() {
            MathTextKind::Grapheme(text) => Ok(SymbolElem::packed(text.clone())),
            MathTextKind::Number(text) => Ok(TextElem::packed(text.clone())),
        }
    }
}

impl Eval for ast::MathIdentWrapper<'_> {
    type Output = Value;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        match self.inner() {
            MathAccess::Ident(ident) => super::code::eval_ident(vm, ident, true),
            MathAccess::FieldAccess(field_access) => field_access.eval(vm),
        }
    }
}

pub(super) fn eval_ident_wrapper(
    vm: &mut Vm,
    wrapper: ast::MathIdentWrapper,
    is_callee: bool,
) -> SourceResult<Value> {
    let value = match wrapper.inner() {
        MathAccess::Ident(ident) => super::code::eval_ident(vm, ident, true),
        MathAccess::FieldAccess(field_access) => field_access.eval(vm),
    }?;
    // Produce an error  for function literals that aren't being called.
    if !is_callee
        && !matches!(value, Value::Symbol(_))
        && value.clone().cast::<Func>().is_ok()
    {
        let func = wrapper.to_untyped().clone().into_text();
        bail!(
            wrapper.span(),
            "this does not call the `{func}` function";
            hint: "to call the `{func}` function, write `{func}()`"
            // TODO: Hint to remove a space if followed by non-direct parens: `abs ()`.
        );
    }
    Ok(value)
}

impl Eval for ast::MathShorthand<'_> {
    type Output = Value;

    fn eval(self, _: &mut Vm) -> SourceResult<Self::Output> {
        Ok(Value::Symbol(Symbol::runtime_char(self.get())))
    }
}

impl Eval for ast::MathAlignPoint<'_> {
    type Output = Content;

    fn eval(self, _: &mut Vm) -> SourceResult<Self::Output> {
        Ok(AlignPointElem::shared().clone())
    }
}

impl Eval for ast::MathDelimited<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let open = self.open().eval_display(vm)?;
        let body = self.body().eval(vm)?;
        let close = self.close().eval_display(vm)?;
        Ok(LrElem::new(open + body + close).pack())
    }
}

impl Eval for ast::MathAttach<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let base = self.base().eval_display(vm)?;
        let mut elem = AttachElem::new(base);

        if let Some(expr) = self.top() {
            elem.t.set(Some(expr.eval_display(vm)?));
        }

        // Always attach primes in scripts style (not limits style),
        // i.e. at the top-right corner.
        if let Some(primes) = self.primes() {
            elem.tr.set(Some(primes.eval(vm)?));
        }

        if let Some(expr) = self.bottom() {
            elem.b.set(Some(expr.eval_display(vm)?));
        }

        Ok(elem.pack())
    }
}

impl Eval for ast::MathPrimes<'_> {
    type Output = Content;

    fn eval(self, _: &mut Vm) -> SourceResult<Self::Output> {
        Ok(PrimesElem::new(self.count()).pack())
    }
}

impl Eval for ast::MathFrac<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let num = self.num();
        let denom = self.denom();

        // Check for ambiguous notation from implicit function calls.
        let mut hints: Vec<EcoString> = Vec::new();
        let mut bad_trivia = false;
        if let Some((_callee, _delims)) = num.like_func_call {
            hints.push(eco_format!("todo"));
            bad_trivia |= num.trivia_between_slash;
        }
        if let Some((_callee, _delims)) = denom.like_func_call {
            hints.push(eco_format!("todo"));
            bad_trivia |= denom.trivia_between_slash;
        }
        if !hints.is_empty() {
            if bad_trivia {
                // TODO.
            }
            let error = SourceDiagnostic::error(self.span(), "notation is ambiguous")
                .with_hints(hints);
            return Err(eco_vec![error]);
        }

        // Evaluate sides and check for ambiguous notation from actual function calls.
        let frac_span = Some(self.span());
        let num_content = match num.expr {
            ast::Expr::MathCall(call) => {
                crate::call::eval_math_call(vm, call, frac_span)?
                    .display()
                    .spanned(call.span())
            }
            expr => expr.eval_display(vm)?,
        };
        let denom_content = match denom.expr {
            ast::Expr::MathCall(call) => {
                crate::call::eval_math_call(vm, call, frac_span)?
                    .display()
                    .spanned(call.span())
            }
            expr => expr.eval_display(vm)?,
        };

        Ok(FracElem::new(num_content, denom_content)
            .with_num_deparenthesized(num.deparenthesized)
            .with_denom_deparenthesized(denom.deparenthesized)
            .pack())
    }
}

impl Eval for ast::MathRoot<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        // Use `TextElem` to match `MathTextKind::Number` above.
        let index = self.index().map(|i| TextElem::packed(eco_format!("{i}")));
        let radicand = self.radicand().eval_display(vm)?;
        Ok(RootElem::new(radicand).with_index(index).pack())
    }
}

trait ExprExt {
    fn eval_display(&self, vm: &mut Vm) -> SourceResult<Content>;
}

impl ExprExt for ast::Expr<'_> {
    fn eval_display(&self, vm: &mut Vm) -> SourceResult<Content> {
        Ok(self.eval(vm)?.display().spanned(self.span()))
    }
}

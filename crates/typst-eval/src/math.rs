use ecow::{EcoString, EcoVec, eco_format, eco_vec};
use typst_library::diag::{At, SourceDiagnostic, SourceResult, error, warning};
use typst_library::foundations::{
    Content, Func, NativeElement, Symbol, SymbolElem, Value,
};
use typst_library::math::{
    AlignPointElem, AttachElem, EquationElem, FracElem, LrElem, PrimesElem, RootElem,
};
use typst_library::text::TextElem;
use typst_syntax::ast::{self, AstNode, MathTextKind};
use typst_syntax::{DiagSpan, SubRange, SyntaxKind, SyntaxNode};

use crate::{Eval, Vm};

impl Eval for ast::Equation<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let body = self.body().eval(vm)?;
        let block = match self.block() {
            ast::EquationBlock::Consistent { block } => block,
            ast::EquationBlock::Inconsistent => {
                vm.engine.sink.warn(warning!(
                    self.span(),
                    "inconsistent spacing next to opening and closing dollar signs";
                    hint: "a block-level equation requires whitespace both after the \
                           opening dollar sign and before the closing dollar sign";
                    hint: "an inline equation should not have whitespace on either side";
                    hint: "this is being treated as an inline equation";
                ));
                // We treat inconsistently spaced equations as inline since one
                // of the sides didn't have a space. This avoids shifting the
                // layout when writing `$a + $` before typing `b`.
                false
            }
        };
        Ok(EquationElem::new(body).with_block(block).pack())
    }
}

impl Eval for ast::Math<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let mut expr_offsets = self.expr_offsets();
        let iter = std::iter::from_fn(move || {
            let (expr, expr_start) = expr_offsets.next()?;
            Some(expr.eval_display(vm, None).map_err(|math_error| {
                match math_error {
                    MathError::Normal(err) => err,
                    MathError::FuncLiteral { node, name } => {
                        // Add a custom hint if the error was due to a function
                        // literal followed by delimiters.
                        let mut overall_span = None;
                        let delims = expr_offsets
                            .find(|(expr, _)| !matches!(expr, ast::Expr::Space(_)))
                            .and_then(|(non_space, offset)| {
                                let ast::Expr::MathDelimited(delims) = non_space else {
                                    return None;
                                };
                                let end = offset + delims.to_untyped().len();
                                overall_span = Some(DiagSpan::from_span(
                                    self.span(),
                                    SubRange::new(expr_start, end),
                                ));
                                Some(delims)
                            });
                        eco_vec![func_literal_error(node, name, delims, overall_span)]
                    }
                }
            }))
        });
        Ok(Content::sequence(iter.collect::<SourceResult<Vec<_>>>()?))
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

impl Eval for ast::MathIdent<'_> {
    type Output = Value;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let span = self.span();
        Ok(vm
            .scopes
            .get_in_math(&self)
            .at(span)?
            .read_checked((&mut vm.engine, span))
            .clone())
    }
}

impl Eval for ast::MathFieldAccess<'_> {
    type Output = Value;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let target = self.target().eval(vm)?;
        let field = self.field();
        crate::code::access_field(vm, target, field.as_str(), field.span())
    }
}

impl Eval for ast::MathAccess<'_> {
    type Output = Value;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        let value = match self {
            ast::MathAccess::MathIdent(ident) => ident.eval(vm)?,
            ast::MathAccess::MathFieldAccess(access) => access.eval(vm)?,
        };
        // We need to call `trace_at` for the value because we did not evaluate
        // via `ast::Expr::eval()`.
        vm.trace_at(self.span(), &value);
        Ok(value)
    }
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
        let open = self.open().eval_display(vm, None)?;
        let body = self.body().eval(vm)?;
        let close = self.close().eval_display(vm, None)?;
        Ok(LrElem::new(open + body + close).pack())
    }
}

impl Eval for ast::MathAttach<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        /// Replaces an attached piece of content in-place with an `AttachElem`
        /// containing that content as the base.
        ///
        /// This allows us to add chained attachments like `a_1_2_3` as
        /// right-associative `a_(1_(2_(3)))` while iterating from left to
        /// right.
        fn replace_as_attach(attached: &mut Option<Content>) -> &mut AttachElem {
            let base = attached.take().unwrap();
            let elem = AttachElem::new(base).pack();
            attached.insert(elem).to_packed_mut().unwrap()
        }

        let base = self.base().eval_display(vm, Some(Operand::AttachBase(self)))?;

        // Prepare mutable pointers to attachment positions of the base.
        let mut attach = AttachElem::new(base);
        let mut b = attach.b.as_option_mut();
        // `t` and `tr_primes` must always refer to the same attachment.
        let mut t = attach.t.as_option_mut();
        let mut tr_primes = attach.tr.as_option_mut();

        for attachment in self.attachments() {
            match attachment {
                ast::Attachment::Bot(expr) => {
                    if let Some(outer_b) = b {
                        let bot = replace_as_attach(outer_b);
                        b = bot.b.as_option_mut();
                    }
                    *b = Some(Some(expr.eval_display(vm, None)?));
                }
                ast::Attachment::Top(expr) => {
                    // we only check for a present `t` here, not `tr_primes`
                    // since it's fine to have both on the same attachment so
                    // long as `tr_primes` came first. Attachments try to merge
                    // `tr` primes and `t` as `tr + t` in the math IR, but
                    // adding `tr + t` is only the correct order if the `tr`
                    // primes came first.
                    if let Some(outer_t) = t {
                        let top = replace_as_attach(outer_t);
                        // But we do update `tr_primes` if we replace `t` so
                        // they always refer to the same attachment.
                        t = top.t.as_option_mut();
                        tr_primes = top.tr.as_option_mut();
                    }
                    *t = Some(Some(expr.eval_display(vm, None)?));
                }
                ast::Attachment::Primes(primes) => {
                    let count = primes.count();
                    if let (None, Some(Some(prev_primes))) = (&mut t, &mut tr_primes) {
                        // Merge adjacent primes into a single `PrimesElem` with
                        // the span of the initial primes. Primes can only be
                        // adjacent in this way if separated by bottom
                        // attachments, like `$a'_b''_c'$`.
                        let primes_elem =
                            prev_primes.to_packed_mut::<PrimesElem>().unwrap();
                        primes_elem.count += count;
                    } else {
                        // We attach primes to `tr`, but still overwrite any
                        // present `t` so that we never add `tr_primes` when a
                        // `t` came first, as attachments would swap their order
                        // when trying to merge `tr` primes and `t` as `tr + t`
                        // in the math IR.
                        if let Some(outer_t) = t {
                            let top = replace_as_attach(outer_t);
                            // `t` and `tr_primes` must always refer to the same
                            // attachment.
                            t = top.t.as_option_mut();
                            tr_primes = top.tr.as_option_mut();
                        } else {
                            assert!(tr_primes.is_none());
                        }
                        *tr_primes = Some(Some(
                            PrimesElem::new(count).pack().spanned(primes.span()),
                        ));
                    }
                }
            }
        }

        Ok(attach.pack())
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
        let num_expr = self.num();
        let num = num_expr.eval_display(vm, Some(Operand::Numerator(self)))?;
        let denom_expr = self.denom();
        let denom = denom_expr.eval_display(vm, Some(Operand::Denominator(self)))?;

        let num_depar =
            matches!(num_expr, ast::Expr::Math(math) if math.was_deparenthesized());
        let denom_depar =
            matches!(denom_expr, ast::Expr::Math(math) if math.was_deparenthesized());

        Ok(FracElem::new(num, denom)
            .with_num_deparenthesized(num_depar)
            .with_denom_deparenthesized(denom_depar)
            .pack())
    }
}

impl Eval for ast::MathRoot<'_> {
    type Output = Content;

    fn eval(self, vm: &mut Vm) -> SourceResult<Self::Output> {
        // Use `TextElem` to match `MathTextKind::Number` above.
        let index = self.index().map(|i| TextElem::packed(eco_format!("{i}")));
        let radicand = self.radicand().eval_display(vm, Some(Operand::Root(self)))?;
        Ok(RootElem::new(radicand).with_index(index).pack())
    }
}

#[expect(unused, reason = "TODO: sub-ranges?")]
pub(crate) enum Operand<'a> {
    AttachBase(ast::MathAttach<'a>),
    Numerator(ast::MathFrac<'a>),
    Denominator(ast::MathFrac<'a>),
    Root(ast::MathRoot<'a>),
}

trait ExprExt<'a> {
    fn eval_display(
        self,
        vm: &mut Vm,
        parent_op: Option<Operand>,
    ) -> Result<Content, MathError<'a>>;
}

impl<'a> ExprExt<'a> for ast::Expr<'a> {
    /// Evaluate the expression as content for math.
    fn eval_display(
        self,
        vm: &mut Vm,
        parent_op: Option<Operand>,
    ) -> Result<Content, MathError<'a>> {
        let value = if parent_op.is_some()
            && let ast::Expr::MathCall(math_call) = self
        {
            crate::call::eval_math_call(vm, math_call, parent_op)?
        } else {
            self.eval(vm)?
        };

        // Symbols can cast to functions, but we don't error since they're also
        // valid as content.
        if !matches!(value, Value::Symbol(_))
            && let Ok(func_value) = value.clone().cast::<Func>()
        {
            return Err(MathError::FuncLiteral {
                node: self.to_untyped(),
                name: func_value.name().map(|name| name.into()),
            });
        }
        Ok(value.display().spanned(self.span()))
    }
}

/// An error wrapper that allows adding custom hints for function literals
/// displayed in math.
pub(crate) enum MathError<'a> {
    /// A normal source error.
    Normal(EcoVec<SourceDiagnostic>),
    /// An attempt to display a function literal in math.
    FuncLiteral { node: &'a SyntaxNode, name: Option<EcoString> },
}

impl From<EcoVec<SourceDiagnostic>> for MathError<'_> {
    fn from(value: EcoVec<SourceDiagnostic>) -> Self {
        Self::Normal(value)
    }
}

impl From<MathError<'_>> for EcoVec<SourceDiagnostic> {
    fn from(value: MathError) -> Self {
        match value {
            MathError::Normal(err) => err,
            MathError::FuncLiteral { node, name } => {
                eco_vec![func_literal_error(node, name, None, None)]
            }
        }
    }
}

/// Error for a function literal in math, potentially with hints for following
/// delimiters.
#[cold]
fn func_literal_error(
    node: &SyntaxNode,
    name: Option<EcoString>,
    delims: Option<ast::MathDelimited>,
    overall_span: Option<DiagSpan>,
) -> SourceDiagnostic {
    let func;
    let mut error;
    match node.kind() {
        // Identifier-like kinds that are reasonable to give custom hints.
        // Normal field access isn't worth handling.
        SyntaxKind::Ident | SyntaxKind::MathIdent | SyntaxKind::MathFieldAccess => {
            func = node.full_text();
            let span = overall_span.unwrap_or(node.span().into());
            error = error!(span, "this does not call the `{func}` function");
        }
        kind => {
            error = error!(node.span(), "expected content, found function");
            if let Some(name) = name {
                error.hint(eco_format!("evaluated to the `{name}` function"));
            }
            if kind == SyntaxKind::MathCall {
                // `MathCall` is the only kind that can produce a function
                // literal but cannot be called by adding trailing parentheses
                // (writing `$func()()$` doesn't work), so we just return
                // without adding extra hints.
                return error;
            }
            func = node.full_text();
        }
    }

    match delims {
        None => error.hint(eco_format!(
            "to call the function, specify arguments in parentheses: `{func}()`"
        )),
        Some(delims) => {
            if let ast::Expr::MathText(open) = delims.open()
                && let ast::Expr::MathText(close) = delims.close()
                && open.to_untyped().leaf_text() == "("
                && close.to_untyped().leaf_text() == ")"
            {
                error.hint(eco_format!(
                    "to call the function, write `{func}{}`",
                    delims.to_untyped().full_text()
                ));
                error.spanned_hint(
                    "the parentheses must directly follow the function",
                    delims.span(),
                );
            } else {
                error.hint(eco_format!(
                    "to call the function, write `{func}({})`",
                    delims.body().to_untyped().full_text()
                ));
                error.spanned_hint(
                    "functions can only be called with matched parentheses",
                    delims.span(),
                );
            }
        }
    }

    error
}

//! Tokenize unparsed math text.
use std::ops::ControlFlow;

use ecow::{EcoString, EcoVec, eco_vec};
use typst_library::diag::{SourceDiagnostic, SourceResult, bail};
use typst_library::foundations::{Args, Content, Func, Value};
use typst_syntax::Span;
use typst_syntax::ast::{MathKind, MathTokenNode, TokenCursor};

use crate::{Eval, Vm, call};

/// A token stream with a type-safe interface for parsing and evaluating tokens
/// and managing errors.
pub struct TokenStream<'ast, 'vm, 'a> {
    vm: &'vm mut Vm<'a>,
    cursor: TokenCursor<'ast>,
    mode: Mode,
    next: Option<TokenInfo>,
    errors: EcoVec<SourceDiagnostic>,
}

/// The token stream's lexing mode. Causes the stream to return a false `None`
/// value when the next token would end the current mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Arguments ends at any RightParen, Comma, or Semicolon.
    Args,
    /// Delimiters ends at any `MathKind::Closing`.
    Delims,
}

/// Information about a token used by the parser and the token stream.
#[derive(Debug)]
pub struct TokenInfo {
    pub token: Token,
    pub trivia: Trivia,
    mark: Marker,
}

/// An evaluated token.
#[derive(Debug, Clone)]
pub enum Token {
    Value(Value),
    FuncCall(Func),
    Kind(MathKind, EcoString),
    ArgStart(ArgStart),
}

/// Tokens that cause special behavior at the start of arguments. Will only be
/// generated in [`Mode::Args`].
///
/// This is split out as a separate struct for use in argument parsing.
#[derive(Debug, Clone)]
pub enum ArgStart {
    Spread,
    NamedArg { name: EcoString },
}

/// Information about trivia preceding a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trivia {
    /// No trivia; the token directly follows its prior.
    Direct,
    /// Trivia exists, but only comments. Note that this would require the
    /// comments to all be block comments.
    OnlyComments,
    /// The trivia contains spaces, which may cause us to insert a space in the
    /// content sequence whose span is the first space's span.
    HasSpaces { span: Span },
}

/// A marker with the token's initial span and overall length.
#[derive(Debug)]
pub struct Marker {
    pub span: Span,
}

impl<'ast, 'vm, 'a> TokenStream<'ast, 'vm, 'a> {
    /// Create a new token stream from a cursor and the vm.
    pub fn new(vm: &'vm mut Vm<'a>, mut cursor: TokenCursor<'ast>) -> Self {
        let mut errors = eco_vec![];
        let next = lex_past_trivia(vm, &mut errors, &mut cursor, false);
        Self { vm, cursor, mode: Mode::Normal, next, errors }
    }

    /// Finish the token stream by converting a final value into spanned content
    /// or the errors if any happened.
    pub fn finish(self, value: Value, span: Span) -> SourceResult<Content> {
        assert!(self.next.is_none());
        if self.errors.is_empty() {
            Ok(value.display().spanned(span))
        } else {
            Err(self.errors)
        }
    }

    /// Call a function using the vm and reporting any errors. Returns a default
    /// value if errors occured.
    pub fn call_func(
        &mut self,
        func: Func,
        args: Args,
        (start, _end): (Marker, Marker),
    ) -> Value {
        match call::call_func(self.vm, func, args, start.span) {
            Ok(value) => value.spanned(start.span),
            Err(diag_vec) => {
                self.errors.extend(diag_vec);
                Value::default()
            }
        }
    }

    /// Produce an error at the given marker.
    pub fn error_at(&mut self, mark: Marker, message: impl Into<EcoString>) {
        let Marker { span } = mark;
        let diag = SourceDiagnostic::error(span, message);
        self.errors.push(diag);
    }

    /// Produce an error from the given marker up to (excluding) the next token.
    pub fn error_from(&mut self, mark: Marker, message: impl Into<EcoString>) {
        let Marker { span } = mark;
        let diag = SourceDiagnostic::error(span, message);
        self.errors.push(diag);
    }

    /// Returns the next token plus a closure for confirming it and advancing
    /// the stream forward.
    ///
    /// This is the main inteface for the token stream because it allows
    /// inspecting the next token without deciding to keep it (by not calling
    /// the closure), while the borrow checker ensures the stream doesn't
    /// change until the token is confirmed or the confirmation is cancelled.
    pub fn peek_with_confirm<'x>(
        &'x mut self,
    ) -> Option<((Token, Trivia), impl FnOnce() -> Marker + use<'x, 'vm, 'a, 'ast>)> {
        self.just_peek().map(|peek| (peek, || self.advance()))
    }

    /// Peek the next token with no option to confirm.
    ///
    /// Note that this takes `self` by non-mutable reference.
    pub fn just_peek(&self) -> Option<(Token, Trivia)> {
        if self.at_mode_end().is_some() {
            return None;
        }
        self.next.as_ref().map(|info| (info.token.clone(), info.trivia))
    }

    /// Advance the stream if we're at a specific mode-ending character.
    pub fn advance_if_at(&mut self, c: char) -> bool {
        let at_char = self.at_mode_end() == Some(c);
        if at_char {
            self.advance();
        }
        at_char
    }

    /// Enter a new mode and call the given function. Returns the final marker
    /// and the mode-ending character unless we encountered the end of the token
    /// stream itself.
    ///
    /// This allows us to emulate a stack of modes using the call stack itself!
    pub fn enter_mode<T>(
        &mut self,
        mode: Mode,
        func: impl FnOnce(&mut Self) -> T,
    ) -> (T, Option<(char, Marker)>) {
        let previous = self.mode;
        self.mode = mode;
        let value = func(self);
        let mode_end = self.at_mode_end();
        self.mode = previous;
        assert!(mode_end.is_some() || self.next.is_none());
        let end_info = mode_end.map(|c| (c, self.advance()));
        (value, end_info)
    }

    /// Returns the character of the next token if it ends our current mode.
    fn at_mode_end(&self) -> Option<char> {
        match (self.mode, &self.next.as_ref()?.token) {
            (Mode::Normal, _) => None,
            (Mode::Delims, Token::Kind(MathKind::Closing(c), _)) => Some(*c),
            (Mode::Args, Token::Kind(MathKind::Closing(')'), _)) => Some(')'),
            (Mode::Args, Token::Kind(MathKind::Comma, _)) => Some(','),
            (Mode::Args, Token::Kind(MathKind::Semicolon, _)) => Some(';'),
            _ => None,
        }
    }

    /// Advance the parser unconditionally. Assumes that `next` has already been
    /// verified as `Some`.
    fn advance(&mut self) -> Marker {
        let previous = self.next.take().unwrap();

        let at_arg_start = match previous.token {
            Token::FuncCall(_) => true,
            Token::Kind(MathKind::Comma | MathKind::Semicolon, _) => {
                self.mode == Mode::Args
            }
            _ => false,
        };
        self.next =
            lex_past_trivia(self.vm, &mut self.errors, &mut self.cursor, at_arg_start);

        previous.mark
    }
}

/// Lex the next token using [`ControlFlow`] to skip trivia.
fn lex_past_trivia(
    vm: &mut Vm,
    errors: &mut EcoVec<SourceDiagnostic>,
    cursor: &mut TokenCursor,
    at_arg_start: bool,
) -> Option<TokenInfo> {
    let mark;
    let mut trivia = Trivia::Direct;
    let token = loop {
        let (node_token, span) = cursor.advance()?;
        match lex_and_eval(vm, cursor, node_token, span, at_arg_start) {
            Err(err_vec) => {
                // Note: in all current error cases we only use the original
                // token, so we don't update the cursor here.
                errors.extend(err_vec);
                mark = Marker { span };
                break Token::Value(Value::default());
            }
            Ok(ControlFlow::Break((token, n))) => {
                cursor.confirm(n);
                mark = Marker { span };
                break token;
            }
            // Skip trivia preceding real tokens and continue the loop.
            Ok(ControlFlow::Continue(is_space)) => match trivia {
                Trivia::OnlyComments | Trivia::Direct if is_space => {
                    trivia = Trivia::HasSpaces { span };
                }
                Trivia::Direct => trivia = Trivia::OnlyComments,
                _ => {}
            },
        }
    };
    Some(TokenInfo { mark, trivia, token })
}

/// Lex the next token for the stream based on the math token node.
///
/// We return the number of extra nodes used by the token for the caller to
/// confirm in the cursor.
fn lex_and_eval<'a>(
    vm: &mut Vm,
    cursor: &TokenCursor<'a>,
    node_token: MathTokenNode<'a>,
    span: Span,
    at_arg_start: bool,
) -> SourceResult<ControlFlow<(Token, usize), bool>> {
    let mut n_nodes = 0;
    let token = match node_token {
        MathTokenNode::Trivia { is_space } => return Ok(ControlFlow::Continue(is_space)),
        MathTokenNode::ParsedCode(code) => Token::Value(code.eval(vm)?),
        MathTokenNode::ParsedExpr(expr) => Token::Value(expr.eval(vm)?),
        MathTokenNode::FieldAccess(fields) => {
            let value = fields.eval(vm)?;
            if let Some((func, n)) = maybe_func_call(cursor, &value, span) {
                n_nodes = n;
                Token::FuncCall(func)
            } else {
                Token::Value(value)
            }
        }
        MathTokenNode::MathIdent(ident) => {
            // First, try to lex a named function argument. This must happen
            // before we try to evaluate the identifier.
            if at_arg_start
                && let Some((name, n)) = maybe_named_arg(cursor, ident.get(), span)?
            {
                let token = Token::ArgStart(ArgStart::NamedArg { name });
                return Ok(ControlFlow::Break((token, n)));
            }
            let value = ident.eval(vm)?;
            if let Some((func, n)) = maybe_func_call(cursor, &value, span) {
                n_nodes = n;
                Token::FuncCall(func)
            } else {
                Token::Value(value)
            }
        }
        MathTokenNode::Kinds(kind, text) => {
            if at_arg_start
                && matches!(
                    kind,
                    MathKind::Text { ident_like: true, .. }
                        | MathKind::Minus
                        | MathKind::Underscore
                )
                && let Some((name, n)) = maybe_named_arg(cursor, text, span)?
            {
                n_nodes = n;
                Token::ArgStart(ArgStart::NamedArg { name })
            } else if at_arg_start && let Some(n) = maybe_spread(cursor, kind) {
                n_nodes = n;
                Token::ArgStart(ArgStart::Spread)
            } else {
                Token::Kind(kind, text.clone())
            }
        }
    };
    Ok(ControlFlow::Break((token, n_nodes)))
}

/// Try to lex a function call by recognizing an opening paren. The returned
/// length includes the paren.
fn maybe_func_call(
    cursor: &TokenCursor,
    value: &Value,
    span: Span,
) -> Option<(Func, usize)> {
    let mut n = 0;
    if matches!(
        cursor.lookahead(&mut n),
        Some(MathTokenNode::Kinds(MathKind::Opening('('), _))
    ) && let Ok(func) = value.clone().cast::<Func>()
    {
        return Some((func.spanned(span), n));
    }
    None
}

/// Try to lex multiple tokens as a single named argument. The returned length
/// includes the colon.
fn maybe_named_arg(
    cursor: &TokenCursor,
    text: &EcoString,
    span: Span,
) -> SourceResult<Option<(EcoString, usize)>> {
    let mut name = text.clone();
    let mut n = 0;
    loop {
        match cursor.lookahead(&mut n) {
            Some(MathTokenNode::MathIdent(math_ident)) => name.push_str(math_ident.get()),
            Some(MathTokenNode::Kinds(
                MathKind::Text { ident_like: true, .. }
                | MathKind::Minus
                | MathKind::Underscore,
                text,
            )) => name.push_str(text),
            Some(MathTokenNode::Kinds(MathKind::Colon, _)) => break,
            _ => return Ok(None),
        }
    }
    if name == "_" {
        // Disallow plain underscore, it can never be an actual parameter name.
        bail!(span, "expected identifier, found underscore");
    } else {
        Ok(Some((name, n)))
    }
}

/// Try to lex a spread operator if we're at dots and the following tokens don't
/// end the function argument. The returned length includes both dots.
fn maybe_spread(cursor: &TokenCursor, kind: MathKind) -> Option<usize> {
    let mut fake_n = 0;
    if let MathKind::Dot = kind
        && let Some(MathTokenNode::Kinds(MathKind::Dot, _)) = cursor.lookahead(&mut fake_n)
        && let Some(peek) = cursor.lookahead(&mut fake_n) // This mutation is NOT confirmed.
        && !matches!(
            peek,
            MathTokenNode::Kinds(
                MathKind::Semicolon | MathKind::Comma | MathKind::Closing(')'),
                _,
            ) | MathTokenNode::Trivia { .. },
        )
    {
        Some(1) // dots always just use 1 node.
    } else {
        None
    }
}

//! Parse math tokens into Typst values.

use ecow::{EcoString, EcoVec, eco_format};
use indexmap::IndexMap;
use indexmap::map::Entry;
use typst_library::foundations::{
    Arg, Args, Content, Func, IntoValue, NativeElement, Str, SymbolElem, Value,
};
use typst_library::math::{AttachElem, FracElem, LrElem, PrimesElem, RootElem};
use typst_library::text::{SpaceElem, TextElem};
use typst_syntax::ast::MathKind;
use typst_syntax::{Span, Spanned};

use crate::math_parser::tokens::ArgStart;

use super::tokens::{Marker, Mode, Token, TokenStream, Trivia};

/// Parse a math token stream into a single value.
pub fn parse(tokens: &mut TokenStream) -> Value {
    math_expression(tokens, Side::Closed, &[])
        .map(|spanned| spanned.v) // The overall span is handled by our caller.
        .unwrap_or_else(|| Content::empty().into_value())
}

/// A math typesetting operator.
#[derive(Debug)]
struct Operator {
    /// Whether the operator needs a left operand.
    left: Side,
    /// Whether the operator needs a right operand.
    right: Side,
    /// How to finish the operator and produce a value.
    finish: Finish,
}

/// Precedence of an operator side. Isomorphic to an Option, but with specific
/// semantic meaning.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Side {
    Closed,
    Open(Prec),
}

impl Side {
    /// An operator on the left is tighter if it is `Open` and has a strictly
    /// greater precedence than an operator on the right.
    fn tighter_than(&self, right: Prec) -> bool {
        // If left and right are the same, the operator acts right-associative.
        matches!(self, Side::Open(left) if *left > right)
    }
}

/// Operator precedence.
type Prec = u8;

/// Precedence of the implied juxtaposition operator.
const JUXT_PREC: Prec = 0;

/// Ways to finish an operator to produce a value.
#[derive(Debug)]
enum Finish {
    Value { value: Value },
    Expression { expr: ExprKind },
    MaybeChain { expr: ExprKind, with: &'static [MathKind] },
    Delims { left: char },
    ParseFuncArgs { func: Func },
}

/// Types of operator expressions.
#[derive(Debug)]
enum ExprKind {
    Juxtapose,
    Attach { primes: Option<u32>, to: AttachTo },
    Root { index: Option<char> },
    Frac,
    Bang,
}

/// Records the order we encounter `^` or `_` in an attachment chain.
#[derive(Debug)]
enum AttachTo {
    Neither,
    Top,
    Bot,
    TopBot,
    BotTop,
}

/// A standard pratt parser that additionally parses juxtaposed elements,
/// chained sub/superscript operators, and parentheses removal.
fn math_expression(
    tokens: &mut TokenStream,
    left_parent: Side,
    chain_kinds: &[MathKind],
) -> Option<Spanned<Value>> {
    // TODO: If we pass in a `&mut Vec` as a param, we can just use one vector as an arena.
    let mut parsed: Vec<Value> = Vec::new();
    let mut lhs_start = 0;
    let mut juxt = Side::Closed;
    let mut active_chain: Option<ExprKind> = None;
    let mut initial_span = Span::detached();

    loop {
        let Some(((token, trivia), confirm)) = tokens.peek_with_confirm() else {
            break;
        };
        if at_stop(&token, chain_kinds) {
            break;
        }
        let has_lhs = !parsed.is_empty();
        let op = math_op(token, trivia, has_lhs, active_chain.take());

        let mark;
        (lhs_start, mark) = match (has_lhs, op.left) {
            // Nothing, but expected nothing. Yay.
            (false, Side::Closed) => (0, confirm()),
            // Closed but with a lhs, we infer the juxtaposition operator.
            (true, Side::Closed) => {
                let mark = if juxt != Side::Closed {
                    // Already inferred :)
                    confirm()
                } else if left_parent.tighter_than(JUXT_PREC) {
                    break;
                } else {
                    juxt = Side::Open(JUXT_PREC);
                    confirm()
                };
                // Don't ignore spaces between elements when joining.
                match trivia {
                    Trivia::Direct | Trivia::OnlyComments => {}
                    Trivia::HasSpaces { span } => {
                        let elem = SpaceElem::shared().clone().spanned(span);
                        parsed.push(elem.into_value());
                    }
                }
                (parsed.len(), mark)
            }
            // Oops, precedence too low.
            (_, Side::Open(left_prec)) if left_parent.tighter_than(left_prec) => break,
            // Happy path :)
            (true, Side::Open(left_prec)) => {
                if juxt.tighter_than(left_prec) {
                    assert_ne!(initial_span, Span::detached());
                    let value = finish_expression(
                        ExprKind::Juxtapose,
                        parsed.into_iter(),
                        initial_span,
                    );
                    parsed = vec![value]; // The sequence becomes the operator's lhs.
                    (0, confirm())
                } else {
                    // Otherwise, we give only _one_ lhs value to the operator.
                    (lhs_start, confirm())
                }
            }
            // Sad path :(
            (false, Side::Open(_)) => {
                let mark = confirm();
                tokens.error_at(mark, "expected a value to the left of the operator");
                // Don't try to continue the operator, but we do keep parsing.
                continue;
            }
        };
        if initial_span == Span::detached() {
            initial_span = mark.span;
        }

        if juxt != Side::Closed {
            // Refresh our maybe-chain for juxtaposition.
            active_chain = Some(ExprKind::Juxtapose);
        }

        // Push the operator's right side onto the parsed array.
        if let Side::Open(right_prec) = op.right {
            let kinds = match &op.finish {
                Finish::MaybeChain { expr: _, with: kinds } => *kinds,
                _ => &[],
            };
            let value = math_expression(tokens, Side::Open(right_prec), kinds);
            let Some(value) = value else {
                tokens.error_at(mark, "expected a value to the right of the operator");
                continue;
            };
            parsed.push(value.v);
        }

        // Finish the operator expression!
        let value = match op.finish {
            Finish::Value { value } => value.spanned(mark.span),
            Finish::Expression { expr } => {
                let op_values = parsed.drain(lhs_start..);
                finish_expression(expr, op_values, mark.span)
            }
            Finish::MaybeChain { expr, with: kinds } => {
                if tokens.just_peek().is_some_and(|(token, _)| at_stop(&token, kinds)) {
                    active_chain = Some(expr);
                    continue;
                }
                let op_values = parsed.drain(lhs_start..);
                finish_expression(expr, op_values, mark.span)
            }
            Finish::Delims { left } => parse_delimiters(tokens, left, left_parent, mark),
            Finish::ParseFuncArgs { func } => parse_function(tokens, func, mark),
        };

        // Get our value and prepare the new lhs.
        parsed.push(value);
    }

    let value = if let Some(expr) = active_chain {
        finish_expression(expr, parsed.into_iter(), initial_span)
    } else if let Some(value) = parsed.pop() {
        assert!(parsed.is_empty());
        value
    } else if !left_parent.tighter_than(JUXT_PREC) {
        // If looser than juxtapotion, give back an empty sequence.
        Content::empty().into_value()
    } else {
        return None;
    };
    Some(Spanned::new(value, initial_span))
}

/// Should we stop parsing because our parent expects this kind of token?
fn at_stop(token: &Token, stop_kinds: &[MathKind]) -> bool {
    matches!(token, Token::Kind(kind, _text) if stop_kinds.iter().any(|k| k == kind))
}

/// Determine the operator to use for a Token.
fn math_op(
    token: Token,
    trivia: Trivia,
    has_lhs: bool,
    active_chain: Option<ExprKind>,
) -> Operator {
    let has_direct_lhs = has_lhs && trivia == Trivia::Direct;
    let (kind, text) = match token {
        Token::Kind(kind, text) => (kind, text),
        Token::Value(value) => {
            return Operator {
                left: Side::Closed,
                right: Side::Closed,
                finish: Finish::Value { value },
            };
        }
        Token::FuncCall(func) => {
            return Operator {
                left: Side::Closed,
                right: Side::Closed,
                finish: Finish::ParseFuncArgs { func },
            };
        }
        Token::ArgStart(_) => unreachable!("only generated when parsing function args"),
    };
    match kind {
        // Underscore is a right-associative infix op that chains with Hat.
        MathKind::Underscore => {
            let (chain, expr) = match active_chain {
                Some(ExprKind::Attach { primes, to: AttachTo::Top }) => {
                    (false, ExprKind::Attach { primes, to: AttachTo::TopBot })
                }
                Some(ExprKind::Attach { primes, to: AttachTo::Neither }) => {
                    // No top yet, might continue chain.
                    (true, ExprKind::Attach { primes, to: AttachTo::Bot })
                }
                // Otherwise we'll be starting a new attach with just ourself.
                _ => (true, ExprKind::Attach { primes: None, to: AttachTo::Bot }),
            };
            Operator {
                left: Side::Open(2),
                right: Side::Open(2),
                finish: if chain {
                    Finish::MaybeChain { expr, with: &[MathKind::Hat] }
                } else {
                    Finish::Expression { expr }
                },
            }
        }
        // Hat is a right-associative infix op that chains with Underscore.
        MathKind::Hat => {
            let (chain, expr) = match active_chain {
                Some(ExprKind::Attach { primes, to: AttachTo::Bot }) => {
                    (false, ExprKind::Attach { primes, to: AttachTo::BotTop })
                }
                Some(ExprKind::Attach { primes, to: AttachTo::Neither }) => {
                    // No bot yet, might continue chain.
                    (true, ExprKind::Attach { primes, to: AttachTo::Top })
                }
                // Otherwise we'll be starting a new attach with just ourself.
                _ => (true, ExprKind::Attach { primes: None, to: AttachTo::Top }),
            };
            Operator {
                left: Side::Open(2),
                right: Side::Open(2),
                finish: if chain {
                    Finish::MaybeChain { expr, with: &[MathKind::Underscore] }
                } else {
                    Finish::Expression { expr }
                },
            }
        }
        // Primes are a postfix operator with high precedence that chain with
        // either Hat or Underscore on the right. Hat/Underscore do not
        // themselves chain with Primes.
        MathKind::Primes { count } if has_direct_lhs => Operator {
            left: Side::Open(3),
            right: Side::Closed,
            finish: Finish::MaybeChain {
                expr: ExprKind::Attach { primes: Some(count), to: AttachTo::Neither },
                // Primes never continue a chain, but they can always start one.
                with: &[MathKind::Hat, MathKind::Underscore],
            },
        },
        // If not direct with a lhs, primes still render, but don't form an attachment.
        MathKind::Primes { count } => Operator {
            left: Side::Closed,
            right: Side::Closed,
            finish: Finish::Value {
                value: PrimesElem::new(count as usize).into_value(),
            },
        },
        // Slash is a left-associative infix operator with low precedence.
        MathKind::Slash => Operator {
            left: Side::Open(1),
            right: Side::Open(2),
            finish: Finish::Expression { expr: ExprKind::Frac },
        },
        // Root is a prefix operator with precedence higher than slash.
        MathKind::Root { index } => Operator {
            left: Side::Closed,
            right: Side::Open(2),
            finish: Finish::Expression { expr: ExprKind::Root { index } },
        },
        // We want factorials to group to text, so we also make the exclamation
        // mark a tightly binding operator if there is no leading trivia.
        MathKind::Bang if has_direct_lhs => Operator {
            left: Side::Open(4),
            right: Side::Closed,
            finish: Finish::Expression { expr: ExprKind::Bang },
        },
        // Delimiters.
        MathKind::Opening(left) => Operator {
            left: Side::Closed,
            right: Side::Closed,
            finish: Finish::Delims { left },
        },
        // If there is no operator between tokens, this is an atomic expression
        // which is closed on the left and right. More than one of these in a
        // row will become the juxtaposition operator.
        kind => match kind.render_as_symbol() {
            Some(c) => Operator {
                left: Side::Closed,
                right: Side::Closed,
                finish: Finish::Value { value: SymbolElem::new(c).into_value() },
            },
            None => Operator {
                left: Side::Closed,
                right: Side::Closed,
                finish: Finish::Value { value: TextElem::new(text).into_value() },
            },
        },
    }
}

/// Use our parsed values to finish off the expression.
fn finish_expression(
    expr: ExprKind,
    mut vals: impl Iterator<Item = Value>,
    span: Span,
) -> Value {
    let mut next_content = || vals.next().unwrap().display();

    let content: Content = match expr {
        ExprKind::Juxtapose => {
            let sequence = vals.by_ref().map(Value::display);
            Content::sequence(sequence)
        }
        ExprKind::Bang => {
            let sequence = [next_content(), SymbolElem::packed('!')];
            Content::sequence(sequence)
        }
        ExprKind::Attach { primes, to } => {
            let mut attach = AttachElem::new(next_content());
            // Note: We must construct the attach this way due to the merging
            // system in `typst-library/attach.rs`, which checks if a field was
            // set at all, even if it was set to `None` (otherwise we could use
            // the builder-pattern and the `with_b` etc. functions).
            if let Some(count) = primes {
                attach = attach.with_tr(Some(PrimesElem::new(count as usize).pack()));
            }
            attach = match to {
                AttachTo::Neither => attach,
                AttachTo::Bot => attach.with_b(Some(next_content())),
                AttachTo::Top => attach.with_t(Some(next_content())),
                AttachTo::BotTop => {
                    attach.with_b(Some(next_content())).with_t(Some(next_content()))
                }
                AttachTo::TopBot => {
                    attach.with_t(Some(next_content())).with_b(Some(next_content()))
                }
            };
            attach.pack()
        }
        ExprKind::Frac => {
            let num = next_content();
            let denom = next_content();
            FracElem::new(num, denom).pack()
        }
        ExprKind::Root { index } => {
            let radicand = next_content();
            let index = index.map(|c| TextElem::packed(c).spanned(span));
            RootElem::new(radicand).with_index(index).pack()
        }
    };
    assert!(vals.next().is_none());
    content.spanned(span).into_value()
}

/// Parse delimiters. If just ascii parentheses, might only return the body
/// based on the parent or following operator.
fn parse_delimiters(
    tokens: &mut TokenStream,
    opening: char,
    left_parent: Side,
    mark: Marker,
) -> Value {
    let (body, mode_end) = tokens
        .enter_mode(Mode::Delims, |tokens| math_expression(tokens, Side::Closed, &[]));

    let closing = match mode_end {
        // Remove parentheses if they're being used for grouping.
        Some((')', _)) if opening == '(' && remove_raw_parens(left_parent, tokens) => {
            return body.map_or(Content::empty().into_value(), |b| b.v);
        }
        Some((closing, end_mark)) => {
            Some(SymbolElem::packed(closing).spanned(end_mark.span))
        }
        None => None,
    };
    let opening = SymbolElem::packed(opening).spanned(mark.span);
    let body = if let Some(Spanned { v, span }) = body {
        v.display().spanned(span)
    } else {
        Content::empty()
    };

    let content = if let Some(closing) = closing {
        LrElem::new(Content::sequence([opening, body, closing])).pack()
    } else {
        Content::sequence([opening, body])
    };
    content.spanned(mark.span).into_value()
}

/// Whether to remove raw parens based on our left/right side operators.
fn remove_raw_parens(left_parent: Side, tokens: &TokenStream) -> bool {
    match tokens.just_peek().map(|(tok, triv)| math_op(tok, triv, true, None)) {
        // Always un-paren if our parent binds too tight.
        _ if left_parent.tighter_than(JUXT_PREC) => true,
        // Exceptions: We don't un-paren for these operators to our right.
        Some(Operator {
            left: _,
            right: _,
            finish:
                Finish::MaybeChain { expr: ExprKind::Attach { .. }, .. }
                | Finish::Expression { expr: ExprKind::Attach { .. }, .. }
                | Finish::Expression { expr: ExprKind::Bang, .. },
        }) => false,
        // If the upcoming operator on the right binds too tight.
        Some(Operator { left, .. }) if left.tighter_than(JUXT_PREC) => true,
        // Otherwise, we'll keep the parens.
        _ => false,
    }
}

/// Parse and call a function.
fn parse_function(tokens: &mut TokenStream, func: Func, start: Marker) -> Value {
    let (items, Some((')', end))) = tokens.enter_mode(Mode::Args, parse_args) else {
        tokens.error_at(start, "unclosed delimiter");
        return Value::default();
    };
    let args = Args { span: start.span, items };
    tokens.call_func(func, args, (start, end))
}

/// State for parsing function arguments.
#[derive(Default)]
struct ArgParser {
    /// Positional arguments.
    pos: Vec<Spanned<Value>>,
    /// Named arguments plus whether they came from syntax or from a spread
    /// operator.
    named: IndexMap<Str, (Spanned<Value>, NamedSource)>,
    /// The start index of the non-array args if parsing two-dimensions.
    two_dim_idx: Option<usize>,
}

/// Where did this named argument come from?
enum NamedSource {
    Syntax,
    Spread,
}

impl ArgParser {
    /// At a semicolon, split any positional arguments after `two_dim_idx` into
    /// a new array.
    fn semicolon(&mut self) {
        let idx = self.two_dim_idx.take().unwrap_or(0);
        let array = self.pos.drain(idx..).map(|spanned| spanned.v).collect();
        let value = Spanned::new(Value::Array(array), Span::detached());
        self.pos.push(value);
        self.two_dim_idx = Some(idx + 1);
    }

    /// Consume the arg parser and generate the final arguments array.
    fn finish(mut self) -> EcoVec<Arg> {
        if self.two_dim_idx.is_some_and(|idx| idx != self.pos.len()) {
            self.semicolon();
        }
        self.pos
            .into_iter()
            .map(|value| Arg { span: value.span, name: None, value })
            .chain(self.named.into_iter().map(|(name, (value, _))| Arg {
                span: value.span,
                name: Some(name),
                value,
            }))
            .collect()
    }
}

/// Parse function arguments.
fn parse_args(tokens: &mut TokenStream) -> EcoVec<Arg> {
    let mut args = ArgParser::default();
    let mut got_arg = false;
    loop {
        let Some((peek, confirm)) = tokens.peek_with_confirm() else {
            // If we peek a `None`, that means we encountered a mode-ending
            // token. If comma or semicolon, we keep parsing.
            let semicolon = tokens.advance_if_at(';');
            if !semicolon && !tokens.advance_if_at(',') {
                // Either at the close paren or the end of the token stream.
                return args.finish();
            }
            if !got_arg {
                // Insert empty content if no argument.
                let value = Content::empty().into_value();
                args.pos.push(Spanned::new(value, Span::detached()));
            }
            if semicolon {
                args.semicolon();
            }
            got_arg = false;
            continue;
        };
        got_arg = true;

        let arg_modifier = match peek {
            // If we have an arg start modifier, then forward the token stream
            // by calling `confirm`.
            (Token::ArgStart(arg_kind), _triv) => Some((arg_kind, confirm())),
            _ => {
                // `confirm` holds a mutable ref to `tokens`, so we have to drop
                // it before we can call `math_expression` below.
                drop(confirm);
                None
            }
        };

        let value = math_expression(tokens, Side::Closed, &[]).unwrap();

        match arg_modifier {
            Some((ArgStart::NamedArg { name }, mark)) => {
                add_named_arg(tokens, &mut args, name, value, mark);
            }
            Some((ArgStart::Spread, mark)) => {
                add_spread_arg(tokens, &mut args, value, mark)
            }
            None => args.pos.push(value),
        }
    }
}

fn add_named_arg(
    tokens: &mut TokenStream,
    args: &mut ArgParser,
    name: EcoString,
    value: Spanned<Value>,
    mark: Marker,
) {
    match args.named.entry(name.into()) {
        Entry::Vacant(entry) => {
            entry.insert((value, NamedSource::Syntax));
        }
        Entry::Occupied(mut entry) => match entry.get().1 {
            NamedSource::Syntax => {
                // Only error on duplicates if both came from syntax.
                let msg = eco_format!("duplicate argument: {}", entry.key());
                tokens.error_at(mark, msg);
            }
            NamedSource::Spread => {
                // Otherwise, overwrite the existing value.
                entry.insert((value, NamedSource::Syntax));
            }
        },
    }
}

fn add_spread_arg(
    tokens: &mut TokenStream,
    args: &mut ArgParser,
    Spanned { v: value, span }: Spanned<Value>,
    mark: Marker,
) {
    // We apply the overall value's span to each spread item.
    let with_span = |v| Spanned::new(v, span);
    match value {
        Value::None => {}
        Value::Array(array) => args.pos.extend(array.into_iter().map(with_span)),
        Value::Dict(dict) => {
            for (key, val) in dict {
                match args.named.entry(key) {
                    Entry::Vacant(entry) => {
                        entry.insert((with_span(val), NamedSource::Spread));
                    }
                    Entry::Occupied(mut entry) => {
                        // Only overwrite the value, ignore whether it came
                        // from syntax or spread.
                        entry.get_mut().0 = with_span(val);
                    }
                }
            }
        }
        Value::Args(new_args) => {
            for item in new_args.items {
                match item.name {
                    Some(name) => match args.named.entry(name) {
                        Entry::Vacant(entry) => {
                            entry.insert((item.value, NamedSource::Spread));
                        }
                        Entry::Occupied(mut entry) => {
                            // Only overwrite the value, ignore whether it came
                            // from syntax or spread.
                            entry.get_mut().0 = item.value;
                        }
                    },
                    None => args.pos.push(item.value),
                }
            }
        }
        _ => tokens.error_from(mark, eco_format!("cannot spread {}", value.ty())),
    }
}

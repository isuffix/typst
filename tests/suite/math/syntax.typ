// Test math syntax.

--- math-unicode paged html ---
// Test Unicode math.
$ ∑_(i=0)^ℕ a ∘ b = \u{2211}_(i=0)^NN a compose b $

--- math-shorthands paged ---
// Test a few shorthands.
$ underline(f' : NN -> RR) \
  n |-> cases(
    [|1|] &"if" n >>> 10,
    2 * 3 &"if" n != 5,
    1 - 0 thick &...,
  ) $

--- math-common-symbols paged html ---
// Test common symbols.
$ dot \ dots \ ast \ tilde \ star $

--- issue-2044-invalid-parsed-ident paged html ---
// In this bug, the dot at the end was causing the right parenthesis to be
// parsed as an identifier instead of the closing right parenthesis.
$floor(phi.alt.)$
$floor(phi.alt. )$

--- math-ident-invalid-grapheme-at-end-of-ident eval ---
// The final grapheme in `ident⃝` starts with a letter, but ends with U+20DD
// COMBINING ENCLOSING CIRCLE, which is not part of `xid_continue`.
//
// Instead of including this non-ident character in the identifier, we split the
// tokens in the middle of the grapheme. This matches the behavior of code mode,
// but is mainly for implementation simplicity.
#let ident = "A"
#let eqn = $ ident⃝ $
#test(repr(eqn.body), "sequence([A], [⃝])")

--- issue-4828-math-number-multi-char paged html ---
// Numbers should parse the same regardless of number of characters.
$1/2(x)$ vs. $1/10(x)$

--- math-ambiguity-basic-tight eval ---
// Basic examples of math ambiguity warnings with no spaces around operators.

// Warning: 5-8 notation is ambiguous
// Hint: 3-8 to display `1/2` and `pi` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
$ 1/2pi $

// Warning: 4-6 notation is ambiguous
// Hint: 3-8 to display `√-` and `1/a` separately, add a space: `- 1`
// Hint: 4-6 to display `-` and `1` together, add parentheses: `(-1)`
// Hint: 4-6 the rendered output may not match your intent
$ √-1/a $

// Warning: 5-7 notation is ambiguous
// Hint: 3-9 to display `x^-` and `b/2` separately, add a space: `- b`
// Hint: 5-7 to display `-` and `b` together, add parentheses: `(-b)`
// Hint: 5-7 the rendered output may not match your intent
$ x^-b/2 $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-basic-loose eval ---
// Basic examples of math ambiguity warnings with spaces around the operators.

// Warning: 5-10 notation is ambiguous
// Hint: 5-14 to display `>=` and `(y) / z` separately, add a space: `>= (y)`
// Hint: 5-10 to display `>=` and `(y)` together, add parentheses: `(>=(y))`
// Hint: 5-10 the rendered output may not match your intent
// Hint: 5-10 whitespace around `/` does not affect grouping
$ x >=(y) / z $

// TODO: More examples.

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-no-warning-tight eval ---
// With no space around the operators, we don't produce ambiguity warnings for
// these cases.

$ x+2y $
$ a+b/c*d/e-f/g+h $
$ -1/2, $
$ 1/2!pi $
$ 1/2'pi $
$ f_i(x) $
$ f(x)^2 $
$ 1/a(b)c/2 $
$ 1/a(b)(c)d/2 $

$ √f(x)/a $ // TODO?

$ 1/2√x $ // TODO?

// From `math-frac-precedence`
$ 1/f(x) $
$ zeta(x)/2 $
$ "foo"[|x|]/2 $
$ +[x]/2 $
$ 1(x)/2 $
$ 2[x]/2 $
$ 🏳️‍🌈[x]/2 $
$ (a)b/2 $
$ b(a)[b]/2 $

// From `math-frac-implicit-func`
$ f'(x)/f_pi{x} $
$ sin^2(x)/f_0(x) $
$ f!(x)/g^(-1)(x) $
$ a_\u{2a}[|x}/a_"2a"{x|] $
$ f_pi.alt{x}/f_#math.pi.alt{x} $
$ a(b)_c(d)^e(f)/g(h)'_i(j)' $
$ (x)'(x)'(x)'/(x)'(x)'(x)' $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-no-warning-loose eval ---
// With spaces around the operators, we usually give a warning, but not for
// alignment points or linebreaks.

$ 1/ 2&pi /3 $

$ 1/ 2\ pi /3 $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-eval eval ---
// We only get one warning because the main warning message is duplicated.

// Warning: 15-32 notation is ambiguous
// Hint: 15-32 to display `2` and `x` together, add parentheses: `(2x)`
// Hint: 15-32 to display `1/2` and `x` separately, add a space: `2 x`
// Hint: 15-32 the rendered output may not match your intent
// Hint: 15-32 from index `4` to `6`
#let _ = eval("$ 1/2x + 3y/4 $")

//  TODO: Improve by adding hints to equality check in
// `typst-library/src/engine.rs::Sink::warn`?

--- math-ambiguity-spacing-root-only eval ---
// Warning: 4-7 notation is ambiguous
// Hint: 3-7 to display `√2` and `pi` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
$ √2pi $ // suggest (2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-8 to display `√ 2` and `pi` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` does not affect grouping
$ √ 2pi $ // suggest (2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-8 to display `√√2` and `pi` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
$ √√2pi $ // suggest (2pi) not (√2pi)

// Warning: 6-9 notation is ambiguous
// Hint: 3-9 to display `√√ 2` and `pi` separately, add a space: `2 pi`
// Hint: 6-9 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 6-9 the rendered output may not match your intent
// Hint: 6-9 whitespace around `√` does not affect grouping
$ √√ 2pi $ // suggest (2pi)

// Warning: 5-9 notation is ambiguous
// Hint: 3-9 to display `√ √2` and `pi` separately, add a space: `√2 pi`
// Hint: 5-9 to display `√2` and `pi` together, add parentheses: `(√2pi)`
// Hint: 5-9 the rendered output may not match your intent
// Hint: 5-9 whitespace around `√` does not affect grouping
$ √ √2pi $ // suggest (√2pi)

// Warning: 7-10 notation is ambiguous
// Hint: 3-10 to display `√ √ 2` and `pi` separately, add a space: `2 pi`
// Hint: 7-10 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 7-10 the rendered output may not match your intent
// Hint: 7-10 whitespace around `√` does not affect grouping
$ √ √ 2pi $ // suggest (2pi) not (√ 2pi)

$ √(2)pi $ // fine

// Warning: 5-10 notation is ambiguous
// Hint: 3-10 to display `√ (2)` and `pi` separately, add a space: `(2) pi`
// Hint: 5-10 to display `(2)` and `pi` together, add parentheses: `((2)pi)`
// Hint: 5-10 the rendered output may not match your intent
// Hint: 5-10 whitespace around `√` does not affect grouping
$ √ (2)pi $ // suggest ((2)pi)

$ √√(2)pi $ // fine

// Warning: 6-11 notation is ambiguous
// Hint: 3-11 to display `√√ (2)` and `pi` separately, add a space: `(2) pi`
// Hint: 6-11 to display `(2)` and `pi` together, add parentheses: `((2)pi)`
// Hint: 6-11 the rendered output may not match your intent
// Hint: 6-11 whitespace around `√` does not affect grouping
$ √√ (2)pi $ // suggest ((2)pi)

// Warning: 5-11 notation is ambiguous
// Hint: 3-11 to display `√ √(2)` and `pi` separately, add a space: `√(2) pi`
// Hint: 5-11 to display `√(2)` and `pi` together, add parentheses: `(√(2)pi)`
// Hint: 5-11 the rendered output may not match your intent
// Hint: 5-11 whitespace around `√` does not affect grouping
$ √ √(2)pi $ // suggest (√(2)pi) not ((2)pi)

// Warning: 7-12 notation is ambiguous
// Hint: 3-12 to display `√ √ (2)` and `pi` separately, add a space: `(2) pi`
// Hint: 7-12 to display `(2)` and `pi` together, add parentheses: `((2)pi)`
// Hint: 7-12 the rendered output may not match your intent
// Hint: 7-12 whitespace around `√` does not affect grouping
$ √ √ (2)pi $ // suggest ((2)pi) not (√ (2)pi)

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-spacing-root-slash eval ---
$ √(2)pi/a $ // fine

// Warning: 4-7 notation is ambiguous
// Hint: 3-9 to display `√2` and `pi/a` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
$ √2pi/a $ // suggest (2pi)

// Warning: 4-7 notation is ambiguous
// Hint: 3-10 to display `√2` and `pi /a` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
// Hint: 4-7 whitespace around `/` does not affect grouping
$ √2pi /a $ // suggest (√2pi) not (2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-10 to display `√ 2` and `pi/a` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` does not affect grouping
$ √ 2pi/a $ // suggest (2pi/a) not (2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-11 to display `√ 2` and `pi /a` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` or `/` does not affect grouping
$ √ 2pi /a $ // suggest (2pi)

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-spacing-root-hat eval ---
$ √(2)pi^b $ // fine

// Warning: 4-7 notation is ambiguous
// Hint: 3-9 to display `√2` and `pi^b` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
$ √2pi^b $ // suggest (2pi)

// Warning: 4-7 notation is ambiguous
// Hint: 3-10 to display `√2` and `pi ^b` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
// Hint: 4-7 whitespace around `^` does not affect grouping
$ √2pi ^b $ // suggest (2pi) not (√2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-10 to display `√ 2` and `pi^b` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` does not affect grouping
$ √ 2pi^b $ // suggest (2pi^b)

// Warning: 5-8 notation is ambiguous
// Hint: 3-11 to display `√ 2` and `pi ^b` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` or `^` does not affect grouping
$ √ 2pi ^b $ // suggest (2pi)

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-spacing-root-hat-slash eval ---
$ √(2)pi^c/d $ // fine

// Warning: 4-7 notation is ambiguous
// Hint: 3-11 to display `√2` and `pi^c/d` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
$ √2pi^c/d $ // suggest (2pi) not (2pi^c)

// Warning: 4-9 notation is ambiguous
// Hint: 3-12 to display `√2` and `pi^c /d` separately, add a space: `2 pi^c`
// Hint: 4-9 to display `2` and `pi^c` together, add parentheses: `(2pi^c)`
// Hint: 4-9 the rendered output may not match your intent
// Hint: 4-9 whitespace around `/` does not affect grouping
$ √2pi^c /d $ // suggest (√2pi^c)

// Warning: 4-7 notation is ambiguous
// Hint: 3-12 to display `√2` and `pi ^c/d` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
// Hint: 4-7 whitespace around `^` does not affect grouping
$ √2pi ^c/d $ // suggest (2pi) not (√2pi)

// Warning: 4-7 notation is ambiguous
// Hint: 3-13 to display `√2` and `pi ^c /d` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
// Hint: 4-7 whitespace around `^` does not affect grouping
$ √2pi ^c /d $ // suggest (2pi) not (√2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-12 to display `√ 2` and `pi^c/d` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` does not affect grouping
$ √ 2pi^c/d $ // suggest (2pi^c/d)

// Warning: 5-10 notation is ambiguous
// Hint: 3-13 to display `√ 2` and `pi^c /d` separately, add a space: `2 pi^c`
// Hint: 5-10 to display `2` and `pi^c` together, add parentheses: `(2pi^c)`
// Hint: 5-10 the rendered output may not match your intent
// Hint: 5-10 whitespace around `√` or `/` does not affect grouping
$ √ 2pi^c /d $ // suggest (2pi^c) not (2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-13 to display `√ 2` and `pi ^c/d` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` or `^` does not affect grouping
$ √ 2pi ^c/d $ // suggest (2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-14 to display `√ 2` and `pi ^c /d` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `√` or `^` does not affect grouping
$ √ 2pi ^c /d $ // suggest (2pi)

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-spacing-root-hat-underscore eval ---
// Warning: 4-7 notation is ambiguous
// Hint: 3-11 to display `√2` and `pi^e_f` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
$ √2pi^e_f $ // suggest (2pi) not (2pi^e_f)

// Warning: 4-7 notation is ambiguous
// Hint: 3-12 to display `√2` and `pi^e _f` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
$ √2pi^e _f $ // suggest (2pi) not (√2pi^e)

// Warning: 4-7 notation is ambiguous
// Hint: 3-12 to display `√2` and `pi ^e_f` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
// Hint: 4-7 whitespace around `^` does not affect grouping
$ √2pi ^e_f $ // suggest (2pi) not (√2pi)

// Warning: 4-7 notation is ambiguous
// Hint: 3-13 to display `√2` and `pi ^e _f` separately, add a space: `2 pi`
// Hint: 4-7 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 4-7 the rendered output may not match your intent
// Hint: 4-7 whitespace around `^` does not affect grouping
$ √2pi ^e _f $ // suggest (2pi) not (√2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-12 to display `1/2` and `pi^e_f` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
$ 1/2pi^e_f $ // suggest (2pi) not (2pi^e_f)

// Warning: 5-8 notation is ambiguous
// Hint: 3-13 to display `1/2` and `pi^e _f` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
$ 1/2pi^e _f $ // suggest (2pi) not (1/2pi^e)

// Warning: 5-8 notation is ambiguous
// Hint: 3-13 to display `1/2` and `pi ^e_f` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `^` does not affect grouping
$ 1/2pi ^e_f $ // suggest (2pi) not (1/2pi)

// Warning: 5-8 notation is ambiguous
// Hint: 3-14 to display `1/2` and `pi ^e _f` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
// Hint: 5-8 whitespace around `^` does not affect grouping
$ 1/2pi ^e _f $ // suggest (2pi) not (1/2pi)

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-tight-xid-vs-alpha eval ---
// See `math-frac-precedence-xid-vs-alpha` for more.

// Warning: 5-10 notation is ambiguous
// Hint: 3-10 to display `1/2` and `℘(x)` separately, add a space: `2 ℘(x)`
// Hint: 5-10 to display `2` and `℘(x)` together, add parentheses: `(2℘(x))`
// Hint: 5-10 the rendered output may not match your intent
$ 1/2℘(x) $

$ 1/2Ⓢ(x) $ // fine

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-tight-unary-math-class eval ---
// Warn if a `MathClass=Vary|Unary` character (or the `-` shorthand)
// comes after any operator: `√-1`, `E_¬P`, or `x^+2/±y`.

// Warning: 4-6 notation is ambiguous
// Hint: 3-6 to display `√-` and `1` separately, add a space: `- 1`
// Hint: 4-6 to display `-` and `1` together, add parentheses: `(-1)`
// Hint: 4-6 the rendered output may not match your intent
$ √-1 $

// Warning: 5-7 notation is ambiguous
// Hint: 3-7 to display `E_¬` and `P` separately, add a space: `¬ P`
// Hint: 5-7 to display `¬` and `P` together, add parentheses: `(¬P)`
// Hint: 5-7 the rendered output may not match your intent
$ E_¬P $

// Warning: 5-7 notation is ambiguous
// Hint: 3-9 to display `x^+` and `2/±` separately, add a space: `+ 2`
// Hint: 5-7 to display `+` and `2` together, add parentheses: `(+2)`
// Hint: 5-7 the rendered output may not match your intent
// Warning: 8-10 notation is ambiguous
// Hint: 6-10 to display `2/±` and `y` separately, add a space: `± y`
// Hint: 8-10 to display `±` and `y` together, add parentheses: `(±y)`
// Hint: 8-10 the rendered output may not match your intent
$ x^+2/±y $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-spacing-frac-only eval ---
// Warning: 6-9 notation is ambiguous
// Hint: 3-9 to display `1/ 2` and `pi` separately, add a space: `2 pi`
// Hint: 6-9 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 6-9 the rendered output may not match your intent
// Hint: 6-9 whitespace around `/` does not affect grouping
$ 1/ 2pi $
// Warning: 6-9 notation is ambiguous
// Hint: 3-9 to display `1 /2` and `pi` separately, add a space: `2 pi`
// Hint: 6-9 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 6-9 the rendered output may not match your intent
$ 1 /2pi $

// Warning: 3-6 notation is ambiguous
// Hint: 3-8 to display `2` and `pi/1` separately, add a space: `2 pi`
// Hint: 3-6 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 3-6 the rendered output may not match your intent
$ 2pi/1 $
// Warning: 3-6 notation is ambiguous
// Hint: 3-9 to display `2` and `pi /1` separately, add a space: `2 pi`
// Hint: 3-6 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 3-6 the rendered output may not match your intent
// Hint: 3-6 whitespace around `/` does not affect grouping
$ 2pi /1 $
// Warning: 3-6 notation is ambiguous
// Hint: 3-10 to display `2` and `pi / 1` separately, add a space: `2 pi`
// Hint: 3-6 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 3-6 the rendered output may not match your intent
// Hint: 3-6 whitespace around `/` does not affect grouping
$ 2pi / 1 $

// Warning: 3-5 notation is ambiguous
// Hint: 3-8 to display `1` and `x/ 2` separately, add a space: `1 x`
// Hint: 3-5 to display `1` and `x` together, add parentheses: `(1x)`
// Hint: 3-5 the rendered output may not match your intent
// Warning: 7-9 notation is ambiguous
// Hint: 4-12 to display `x/ 2` and `y /3` separately, add a space: `2 y`
// Hint: 7-9 to display `2` and `y` together, add parentheses: `(2y)`
// Hint: 7-9 the rendered output may not match your intent
// Hint: 7-9 whitespace around `/` does not affect grouping
$ 1x/ 2y /3(pi + x) $
// Warning: 3-5 notation is ambiguous
// Hint: 3-8 to display `1` and `x /2` separately, add a space: `1 x`
// Hint: 3-5 to display `1` and `x` together, add parentheses: `(1x)`
// Hint: 3-5 whitespace around `/` does not affect grouping
// Hint: 3-5 the rendered output may not match your intent
// Warning: 7-9 notation is ambiguous
// Hint: 4-12 to display `x /2` and `y/ 3` separately, add a space: `2 y`
// Hint: 7-9 to display `2` and `y` together, add parentheses: `(2y)`
// Hint: 7-9 the rendered output may not match your intent
// Warning: 11-20 notation is ambiguous
// Hint: 8-20 to display `y/ 3` and `(pi + x)` separately, add a space: `3 (pi + x)`
// Hint: 11-20 to display `3` and `(pi + x)` together, add parentheses: `(3(pi + x))`
// Hint: 11-20 the rendered output may not match your intent
// Hint: 11-20 whitespace around `/` does not affect grouping
$ 1x /2y/ 3(pi + x) $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-frac-factorial eval ---
// Warning: 5-8 notation is ambiguous
// Hint: 3-9 to display `1/2` and `pi!` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
$ 1/2pi! $
// Warning: 3-6 notation is ambiguous
// Hint: 3-9 to display `2` and `pi!/3` separately, add a space: `2 pi`
// Hint: 3-6 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 3-6 the rendered output may not match your intent
$ 2pi!/3 $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-frac-attach eval ---
// TODO: Expand these tests?

// Warning: 5-7 notation is ambiguous
// Hint: 3-9 to display `f_2` and `x/3` separately, add a space: `2 x`
// Hint: 5-7 to display `2` and `x` together, add parentheses: `(2x)`
// Hint: 5-7 the rendered output may not match your intent
$ f_2x/3 $

// Warning: 7-10 notation is ambiguous
// Hint: 3-10 to display `1/x_2` and `pi` separately, add a space: `2 pi`
// Hint: 7-10 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 7-10 the rendered output may not match your intent
$ 1/x_2pi $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-spacing-frac-call eval ---
// Warning: 5-10 notation is ambiguous
// Hint: 3-10 to display `1/2` and `f(x)` separately, add a space: `2 f(x)`
// Hint: 5-10 to display `2` and `f(x)` together, add parentheses: `(2f(x))`
// Hint: 5-10 the rendered output may not match your intent
$ 1/2f(x) $
// Warning: 5-10 notation is ambiguous
// Hint: 3-10 to display `1/2` and `E[x]` separately, add a space: `2 E[x]`
// Hint: 5-10 to display `2` and `E[x]` together, add parentheses: `(2E[x])`
// Hint: 5-10 the rendered output may not match your intent
$ 1/2E[x] $
// Warning: 5-11 notation is ambiguous
// Hint: 3-11 to display `1/2` and `pi(x)` separately, add a space: `2 pi(x)`
// Hint: 5-11 to display `2` and `pi(x)` together, add parentheses: `(2pi(x))`
// Hint: 5-11 the rendered output may not match your intent
$ 1/2pi(x) $
// Warning: 5-11 notation is ambiguous
// Hint: 3-11 to display `1/2` and `Pr[x]` separately, add a space: `2 Pr[x]`
// Hint: 5-11 to display `2` and `Pr[x]` together, add parentheses: `(2Pr[x])`
// Hint: 5-11 the rendered output may not match your intent
$ 1/2Pr[x] $
// Warning: 5-12 notation is ambiguous
// Hint: 3-12 to display `1/2` and `abs(x)` separately, add a space: `2 abs(x)`
// Hint: 5-12 to display `2` and `abs(x)` together, add parentheses: `(2abs(x))`
// Hint: 5-12 the rendered output may not match your intent
$ 1/2abs(x) $

// Warning: 3-8 notation is ambiguous
// Hint: 3-10 to display `2` and `f(x)/1` separately, add a space: `2 f(x)`
// Hint: 3-8 to display `2` and `f(x)` together, add parentheses: `(2f(x))`
// Hint: 3-8 the rendered output may not match your intent
$ 2f(x)/1 $
// Warning: 3-8 notation is ambiguous
// Hint: 3-10 to display `2` and `E[x]/1` separately, add a space: `2 E[x]`
// Hint: 3-8 to display `2` and `E[x]` together, add parentheses: `(2E[x])`
// Hint: 3-8 the rendered output may not match your intent
$ 2E[x]/1 $
// Warning: 3-9 notation is ambiguous
// Hint: 3-11 to display `2` and `pi(x)/1` separately, add a space: `2 pi(x)`
// Hint: 3-9 to display `2` and `pi(x)` together, add parentheses: `(2pi(x))`
// Hint: 3-9 the rendered output may not match your intent
$ 2pi(x)/1 $
// Warning: 3-9 notation is ambiguous
// Hint: 3-11 to display `2` and `Pr[x]/1` separately, add a space: `2 Pr[x]`
// Hint: 3-9 to display `2` and `Pr[x]` together, add parentheses: `(2Pr[x])`
// Hint: 3-9 the rendered output may not match your intent
$ 2Pr[x]/1 $
// Warning: 3-10 notation is ambiguous
// Hint: 3-12 to display `2` and `abs(x)/1` separately, add a space: `2 abs(x)`
// Hint: 3-10 to display `2` and `abs(x)` together, add parentheses: `(2abs(x))`
// Hint: 3-10 the rendered output may not match your intent
$ 2abs(x)/1 $

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-ambiguity-inside-delimiters eval ---
// Warning: 5-8 notation is ambiguous
// Hint: 3-8 to display `1/2` and `pi` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
$(1/2pi)$
// Warning: 5-8 notation is ambiguous
// Hint: 3-8 to display `1/2` and `pi` separately, add a space: `2 pi`
// Hint: 5-8 to display `2` and `pi` together, add parentheses: `(2pi)`
// Hint: 5-8 the rendered output may not match your intent
$(1/2pi$

// Warning: 7-10 notation is ambiguous
// Hint: 5-10 to display `2/3` and `pi` separately, add a space: `3 pi`
// Hint: 7-10 to display `3` and `pi` together, add parentheses: `(3pi)`
// Hint: 7-10 the rendered output may not match your intent
$1+(2/3pi$

// Warning: 5-7 notation is ambiguous
// Hint: 4-7 to display `√1` and `x` separately, add a space: `1 x`
// Hint: 5-7 to display `1` and `x` together, add parentheses: `(1x)`
// Hint: 5-7 the rendered output may not match your intent
$(a√1x)$
// Warning: 5-7 notation is ambiguous
// Hint: 4-7 to display `√1` and `x` separately, add a space: `1 x`
// Hint: 5-7 to display `1` and `x` together, add parentheses: `(1x)`
// Hint: 5-7 the rendered output may not match your intent
$(a√1x$

// Error: 2-9 panicked
#panic() // don't try to render this test

--- math-lr-unmatched-adjacent-op-ambiguity eval ---
// Test whether ambiguous unclosed delimiters near fractions/attachments produce
// errors or ambiguous notation warnings.

// TODO: Add cases that actually warn.

// Error: 6-7 unmatched delimiter
// Hint: 6-7 delimiters must be correctly matched when used for grouping
// Hint: 6-7 try adding a closing delimiter: `)`
// Hint: 6-7 or escape the delimiter with a backslash to display it verbatim: `\(`
$ 1/f(2 (x) $
// Error: 6-7 unmatched delimiter
// Hint: 6-7 delimiters must be correctly matched when used for grouping
// Hint: 6-7 try adding a closing delimiter: `]`
// Hint: 6-7 or escape the delimiter with a backslash to display it verbatim: `\[`
$ 1_g[2 y [x] [] $
$ 1]h/2 $
$ 1⟩i^2 $
// Error: 6-8 unmatched delimiter
// Hint: 6-8 delimiters must be correctly matched when used for grouping
// Hint: 6-8 try adding a closing delimiter: `|]`
// Hint: 6-8 or access the delimiter as a variable: `bracket.l.stroked`
$ 1/j[|2 $

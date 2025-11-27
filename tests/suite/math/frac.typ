// Test fractions.

--- math-frac-baseline paged ---
// Test that denominator baseline matches in the common case.
$ x = 1/2 = a/(a h) = a/a = a/(1/2) $

--- math-frac-paren-removal paged ---
// Test parenthesis removal.
$ (|x| + |y|)/2 < [1+2]/3 $

--- math-frac-large paged ---
// Test large fraction.
$ x = (-b plus.minus sqrt(b^2 - 4a c))/(2a) $

--- math-binom paged ---
// Test binomial.
$ binom(circle, square) $

--- math-binom-multiple paged ---
// Test multinomial coefficients.
$ binom(n, k_1, k_2, k_3) $

--- math-binom-missing-lower paged ---
// Error: 3-13 missing argument: lower
$ binom(x^2) $

--- math-dif paged ---
// Test dif.
$ (dif y)/(dif x), dif/x, x/dif, dif/dif \
  frac(dif y, dif x), frac(dif, x), frac(x, dif), frac(dif, dif) $

--- math-frac-associativity paged ---
// Test associativity.
$ 1/2/3 = (1/2)/3 = 1/(2/3) $

--- math-frac-tan-sin-cos paged ---
// A nice simple example of a simple trig property.
$ tan(x) = sin(x) / cos(x) \
  tan x = (sin x) / (cos x) $

--- math-frac-precedence paged ---
// Test precedence.
$ a_1/b_2, 1/(f(x)), (zeta(x))/2, ("foo"[|x|])/2 \
  1.2/3.7, 2.3^3.4 \
  f [x]/2, phi [x]/2 \
  +[x]/2, 1(x)/2, 2[x]/2, 🏳️‍🌈[x]/2 \
  (a)b/2, b(a)[b]/2 \
  n!/2, 5!/2, n !/2, 1/n!, 1/5! $

--- math-frac-func-call-f-denom paged ---
// Error: 3-11 notation is ambiguous
// Hint: 3-11 todo
$ 1/f(x+1) $

--- math-frac-func-call-f-num paged ---
// Error: 3-11 notation is ambiguous
// Hint: 3-11 todo
$ f(x+1)/1 $

--- math-frac-func-call-f-both paged ---
// Error: 3-16 notation is ambiguous
// Hint: 3-16 todo
// Hint: 3-16 todo
$ f(x+1)/f(x+1) $

--- math-frac-func-call-pi-both paged ---
// Error: 3-18 notation is ambiguous
// Hint: 3-18 to display `pi` and `(x+1)` together, add parentheses: `(pi(x+1))`
// Hint: 3-18 to display `pi` and `(x+1)` separately, add a space: `pi (x+1)`
$ pi(x+1)/pi(x+1) $

--- math-frac-implicit-func-1 paged ---
// Test precedence interactions with implicit function calls.
// Error: 3-12 notation is ambiguous
// Hint: 3-12 todo
$ f'(x) / 1 $

--- math-frac-implicit-func-2 paged ---
// Error: 3-14 notation is ambiguous
// Hint: 3-14 todo
$ 1 / f_pi{x} $

--- math-frac-implicit-func-3 paged ---
// TODO: Error here
$ sin^2(x) / 1 $

--- math-frac-implicit-func-4 paged ---
$ f!(x) / g^(-1)(x) $

--- math-frac-implicit-func-5 paged ---
// Error: 3-19 notation is ambiguous
// Hint: 3-19 todo
$ a_\u{2a}[|x} / 1 $

--- math-frac-implicit-func-6 paged ---
// Error: 3-17 notation is ambiguous
// Hint: 3-17 todo
$ 1 / a_"2a"{x|] $

--- math-frac-implicit-func-7 paged ---
// Error: 3-18 notation is ambiguous
// Hint: 3-18 todo
$ f_pi.alt{x} / 1 $

--- math-frac-implicit-func-8 paged ---
// This is fine.
$ 1 / f_#math.pi.alt{x} $

--- math-frac-implicit-func-9 paged ---
// Error: 3-21 notation is ambiguous
// Hint: 3-21 todo
$ a(b)_c(d)^e(f) / 1 $

--- math-frac-implicit-func-10 paged ---
// TODO: Why isn't this g(...) ?
$ 1 / g(h)'_i(j)' $

--- math-frac-implicit-func-11 paged ---
// TODO: Yikes
$ (x)'(x)'(x)' / (x)'(x)'(x)' $

--- math-frac-gap paged ---
// Test that the gap above and below the fraction rule is correct.
$ sqrt(n^(2/3)) $

--- math-frac-horizontal paged ---
// Test that horizontal fractions look identical to inline math with `slash`
#set math.frac(style: "horizontal")
$ (a / b) / (c / (d / e)) $
$ (a slash b) slash (c slash (d slash e)) $

--- math-frac-horizontal-lr-paren paged ---
// Test that parentheses are in a left-right pair even when rebuilt by a horizontal fraction
#set math.frac(style: "horizontal")
$ (#v(2em)) / n $

--- math-frac-skewed paged ---
// Test skewed fractions
#set math.frac(style: "skewed")
$ a / b,  a / (b / c) $

--- math-frac-horizontal-explicit paged ---
// Test that explicit fractions don't change parentheses
#set math.frac(style: "horizontal")
$ frac(a, (b + c)), frac(a, b + c) $

--- math-frac-horizontal-nonparen-brackets paged ---
// Test that non-parentheses left-right pairs remain untouched
#set math.frac(style: "horizontal")
$ [x+y] / {z} $

--- math-frac-styles-inline paged ---
// Test inline layout of styled fractions
#set math.frac(style: "horizontal")
$a/(b+c), frac(a, b+c, style: "skewed"), frac(a, b+c, style: "vertical")$

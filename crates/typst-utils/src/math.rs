use unicode_math_class::MathClass;

/// Returns the default math class of a character in Typst, if it has one.
///
/// This is determined by the Unicode math class, with some manual overrides.
pub fn default_math_class(c: char) -> Option<MathClass> {
    match c {
        // Better spacing.
        // https://github.com/typst/typst/commit/2e039cb052fcb768027053cbf02ce396f6d7a6be
        ':' => Some(MathClass::Relation),

        // Better spacing when used alongside + PLUS SIGN.
        // https://github.com/typst/typst/pull/1726
        '⋯' | '⋱' | '⋰' | '⋮' => Some(MathClass::Normal),

        // Better spacing.
        // https://github.com/typst/typst/pull/1855
        '.' | '/' => Some(MathClass::Normal),

        // ⊥ UP TACK should not be a relation, contrary to ⟂ PERPENDICULAR.
        // https://github.com/typst/typst/pull/5714
        '\u{22A5}' => Some(MathClass::Normal),

        // Used as a binary connector in linear logic, where it is referred to
        // as "par".
        // https://github.com/typst/typst/issues/5764
        '⅋' => Some(MathClass::Binary),

        // Those overrides should become the default in the next revision of
        // MathClass.txt.
        // https://github.com/typst/typst/issues/5764#issuecomment-2632435247
        '⎰' | '⟅' => Some(MathClass::Opening),
        '⎱' | '⟆' => Some(MathClass::Closing),

        // Both ∨ and ⟑ are classified as Binary.
        // https://github.com/typst/typst/issues/5764
        '⟇' => Some(MathClass::Binary),

        // Arabic comma.
        // https://github.com/latex3/unicode-math/pull/633#issuecomment-2028936135
        '،' => Some(MathClass::Punctuation),

        c => unicode_math_class::class(c),
    }
}

/// The matching delimiter for a character, does not include fences.
pub fn matching_delim(c: char) -> Option<char> {
    match c {
        // Optimize ASCII.
        '(' => Some(')'),
        ')' => Some('('),
        '[' => Some(']'),
        ']' => Some('['),
        '{' => Some('}'),
        '}' => Some('{'),
        // Other unicode blocks (paren technically also fits here)
        c => match default_math_class(c) {
            Some(MathClass::Opening) => char::from_u32(c as u32 + 1),
            Some(MathClass::Closing) => char::from_u32(c as u32 - 1),
            _ => None,
        },
    }
}

/// Whether a character's [`MathClass`] is `Vary` or `Unary`, indicating that it
/// may be used as a prefix operator in math notation.
///
/// This is mildly optimized by only matching on the 11 actual `Vary`/`Unary`
/// codepoints instead of calling into [`default_math_class`].
///
/// Note that ∇ U+2207 NABLA does have bold/italic/sans-serif variants whose
/// math classes are `Alphabetic` instead of `Unary`. Also ∆ U+2206 INCREMENT
/// (`Unary`) differs from Δ U+ 0394 GREEK CAPITAL LETTER DELTA (`Alphabetic`).
pub fn math_class_vary_unary(c: char) -> bool {
    let vary_unary =
        matches!(c, '+' | '¬' | '±' | '∀' | '∁' | '∃' | '∄' | '∆' | '∇' | '−' | '∓');
    debug_assert_eq!(
        vary_unary,
        matches!(default_math_class(c), Some(MathClass::Vary | MathClass::Unary)),
    );
    vary_unary
}

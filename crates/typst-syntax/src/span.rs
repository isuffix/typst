use std::fmt::{self, Debug, Formatter};
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};
use std::ops::Range;

use ecow::{EcoString, eco_format};

use crate::FileId;

/// Defines a range of text in a Typst source file.
///
/// Spans are used throughout the compiler to track which source section an
/// element stems from or an error/warning applies to. Errors and warnings use
/// the [`DiagSpan`] type which can contain either a normal span or a range
/// targeting a location in a non-Typst file, such as a JSON parsing error.
///
/// - The [`.id()`](Self::id) function can be used to get the [`FileId`] for the
///   span and, by extension, its file system path.
/// - The `WorldExt::range` function can be used to map the span to a
///   `Range<usize>`.
///
/// This type is stored compactly in 8 bytes, and is copyable and null-optimized
/// (i.e. `Option<Span>` also takes 8 bytes), but can be expanded for easier
/// usage into the [`SpanKind`] enum via [`Self::get()`].
///
/// Spans internally distinguish between four kinds of values, these are
/// accessible as the [`SpanKind`] or [`DiagSpanKind`] enums via the
/// [`Span::get`] or [`DiagSpan::get`] methods.
/// 1. They can be detached, originating from nowhere or from the compiler
///    itself.
/// 2. They can be numbered values, corresponding to a node in a Typst source
///    file's concrete syntax tree. These are the most common type of Span and
///    explained more below.
/// 3. They can be raw range spans, containing a range of two indices that came
///    from parsing a text as Typst syntax. The file itself is not necessarily a
///    Typst source file. The maximum value for the start/end of these ranges is
///    `2^23-1`, larger values will be saturated.
/// 4. They can be an external start index, used for diagnostics on externally
///    loaded text files. These are only accessible as part of a [`DiagSpan`]
///    which also contains the end index. The maximum value for the start/end of
///    these ranges is `2^46-1`, larger values will be saturated.
///
/// # Numbered spans
/// Typst source files use _numbered spans._ Rather than using byte ranges,
/// which shift a lot as you type, each syntax tree node gets a unique number.
///
/// During editing, the span numbers stay mostly stable, even for nodes behind
/// an insertion. This is not true for simple ranges as they would shift. This
/// allows spans to be used as inputs to memoized functions without hurting
/// cache performance when text is inserted somewhere in the document other than
/// the end.
///
/// Span numbers are ordered in the syntax tree to enable quickly finding the
/// node a known span:
/// - The span number of a parent node is always smaller than the number of any
///   of its children
/// - The span numbers of sibling nodes always increase from left to right
///
/// Combining those guarantees, we have that for siblings in order [A, B, C],
/// the span numbers for node A and _all of A's children_ are less than node B's
/// span number, and the numbers for node C and all of C's children are greater
/// than B's span number.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Span(NonZeroU64);

/// The unique number of a span within its [`Source`](crate::Source). Known to
/// be within the range of `Span::FULL`.
///
/// This is mainly used externally as an input to the
/// [`Source::range`](crate::Source::range) method for efficiently finding the
/// byte range of a span.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SpanNumber(pub(crate) u64);

/// The possible kinds of span.
#[derive(Debug)]
pub enum SpanKind {
    /// A span that does not point into any file.
    Detached,
    /// A numbered span.
    Number { id: FileId, num: SpanNumber },
    /// A raw byte range in a file.
    Range { id: FileId, start: usize, end: usize },
}

/// Saturate a value at a given maximum. Can't use `.min()` since it isn't
/// stable in const :/
const fn saturate(value: usize, max: u64) -> u64 {
    if value as u64 > max { max } else { value as u64 }
}

impl Span {
    /// The full range of numbers available for source file span numbering.
    pub(crate) const FULL: Range<u64> = 2..(1 << 47);

    /// The value reserved for the detached span.
    const DETACHED: Self = Self(NonZeroU64::new(1).unwrap());

    /// The span's internal data is laid out with 16 bits for the file id and 48
    /// bits for the span number:
    /// | 16 bits file id | 48 bits number |
    ///
    /// Possible values for the span number are:
    /// - always non-zero
    /// - Detached: 1 (FileId is 0, otherwise non-zero)
    /// - Typst source file:           2 .. 2^47-1      (one 47-bit number)
    /// - External file start:      2^47 .. 2^47+2^46-1 (one 46-bit number)
    /// - Internal range span: 2^47+2^46 .. 2^48-1      (two 23-bit numbers)
    const NUMBER_BITS: usize = 48;
    const FILE_ID_SHIFT: usize = Self::NUMBER_BITS;
    const NUMBER_MASK: u64 = (1 << Self::NUMBER_BITS) - 1;
    const RANGE_BITS: u64 = 46;
    const RANGE_BASE: u64 = Self::EXTERNAL_BASE + (1 << Self::RANGE_BITS);
    const RANGE_VALUE_BITS: u64 = 23;
    const RANGE_VALUE_MAX: u64 = (1 << Self::RANGE_VALUE_BITS) - 1;
    const EXTERNAL_BASE: u64 = Self::FULL.end;
    const EXTERNAL_VALUE_MAX: u64 = (1 << Self::RANGE_BITS) - 1;

    /// Create a span that does not point into any file.
    pub const fn detached() -> Self {
        Self::DETACHED
    }

    /// Create a new span from a file id and a number.
    ///
    /// Returns `None` if `number` is not contained in `FULL`.
    pub(crate) const fn from_number(id: FileId, number: u64) -> Option<Self> {
        if number < Self::FULL.start || number >= Self::FULL.end {
            return None;
        }
        Some(Self::pack(id, number))
    }

    /// Create a new span from a raw byte range instead of a span number.
    ///
    /// If one of the range's parts exceeds the maximum value (2^23-1), it is
    /// saturated.
    pub(crate) const fn from_range(id: FileId, range: Range<usize>) -> Self {
        let start = saturate(range.start, Self::RANGE_VALUE_MAX);
        let end = saturate(range.end, Self::RANGE_VALUE_MAX);
        let number = (start << Self::RANGE_VALUE_BITS) | end;
        Self::pack(id, Self::RANGE_BASE + number)
    }

    /// Construct from a raw number.
    ///
    /// Should only be used with numbers retrieved via
    /// [`into_raw`](Self::into_raw). Misuse may results in panics, but no
    /// unsafety.
    pub const fn from_raw(v: NonZeroU64) -> Self {
        Self(v)
    }

    /// Pack a file ID and the low bits into a span.
    const fn pack(id: FileId, low: u64) -> Self {
        let bits = ((id.into_raw().get() as u64) << Self::FILE_ID_SHIFT) | low;

        // The file ID is non-zero.
        Self(NonZeroU64::new(bits).unwrap())
    }

    /// Whether the span is detached.
    pub const fn is_detached(self) -> bool {
        self.0.get() == Self::DETACHED.0.get()
    }

    /// The id of the file the span points into.
    ///
    /// Returns `None` if the span is detached.
    pub const fn id(self) -> Option<FileId> {
        // Detached span has only zero high bits, so it will trigger the
        // `None` case.
        match NonZeroU16::new((self.0.get() >> Self::FILE_ID_SHIFT) as u16) {
            Some(v) => Some(FileId::from_raw(v)),
            None => None,
        }
    }

    /// The unique number of the span within its [`Source`](crate::Source).
    pub(crate) const fn number(self) -> u64 {
        self.0.get() & Self::NUMBER_MASK
    }

    /// Unpack the span into the variants of a [`SpanKind`] for easier use.
    ///
    /// To access a range, you may want to use `WorldExt::range` instead.
    pub const fn get(self) -> SpanKind {
        let Some(id) = self.id() else { return SpanKind::Detached };
        let num = self.number();
        if let Some(packed_range) = num.checked_sub(Self::RANGE_BASE) {
            let start = (packed_range >> Self::RANGE_VALUE_BITS) as usize;
            let end = (packed_range & Self::RANGE_VALUE_MAX) as usize;
            SpanKind::Range { id, start, end }
        } else {
            SpanKind::Number { id, num: SpanNumber(num) }
        }
    }

    /// Extract the raw underlying number.
    pub const fn into_raw(self) -> NonZeroU64 {
        self.0
    }

    /// Return `other` if `self` is detached and `self` otherwise.
    pub fn or(self, other: Self) -> Self {
        if self.is_detached() { other } else { self }
    }

    /// Find the first non-detached span in the iterator.
    pub fn find(iter: impl IntoIterator<Item = Self>) -> Self {
        iter.into_iter()
            .find(|span| !span.is_detached())
            .unwrap_or(Span::detached())
    }
}

/// The span of a diagnostic message. Either from a Typst source file or from a
/// loaded external file.
///
/// Typst source spans may additionally contain a sub-range targeting just part
/// of the overall range of the span.
///
/// When storing an external file range, the maximum value of the start/end is
/// `2^46-1`, larger values are saturated.
///
/// This type is stored compactly in 16 bytes and null-optimized, but can be
/// expanded for easier usage into the [`DiagSpanKind`] enum via
/// [`Self::get()`].
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct DiagSpan {
    span: Span,
    extra: u64,
}

/// The possible kinds of a diagnostic span.
#[derive(Debug)]
pub enum DiagSpanKind {
    Detached,
    Number { id: FileId, num: SpanNumber, sub_range: Option<SubRange> },
    Range { id: FileId, start: usize, end: usize },
}

impl DiagSpan {
    /// The id of the file the span points into.
    ///
    /// Returns `None` if the span is detached.
    pub fn id(self) -> Option<FileId> {
        self.span.id()
    }

    /// Create a new diagnostic span from a source span.
    pub(crate) fn new_source(span: Span, sub_range: Option<SubRange>) -> Self {
        let extra = match sub_range {
            Some(SubRange { start, end }) => ((start as u64) << 32) | (end.get() as u64),
            None => 0,
        };
        Self { span, extra }
    }

    /// Create a new diagnostic span from an external file byte range instead of
    /// an internal span.
    ///
    /// If one of the range's parts exceeds the maximum value (2^46-1), it is
    /// saturated.
    pub fn new_range(id: FileId, range: Range<usize>) -> Self {
        let start = saturate(range.start, Span::EXTERNAL_VALUE_MAX);
        let end = saturate(range.end, Span::EXTERNAL_VALUE_MAX);
        Self {
            span: Span::pack(id, Span::EXTERNAL_BASE + start),
            extra: end,
        }
    }

    /// Convert to the easier to use `DiagSpanKind` enum.
    pub fn get(self) -> DiagSpanKind {
        let DiagSpan { span, extra } = self;
        match span.get() {
            SpanKind::Detached => DiagSpanKind::Detached,
            SpanKind::Number { id, num } => {
                if let Some(start) = num.0.checked_sub(Span::EXTERNAL_BASE) {
                    // Note: this checked_sub must come after the internal range check.
                    let start = start as usize;
                    let end = extra as usize;
                    let id = span.id().unwrap();
                    DiagSpanKind::Range { id, start, end }
                } else {
                    let sub_range = {
                        let start = (extra >> 32) as u32;
                        let end = NonZeroU32::new(extra as u32); // `as` does truncation
                        end.map(|end| SubRange { start, end })
                    };
                    DiagSpanKind::Number { id, num, sub_range }
                }
            }
            SpanKind::Range { id, start, end: _ } if extra != 0 => {
                let sub_range = {
                    let start = (extra >> 32) as u32;
                    let end = NonZeroU32::new(extra as u32).unwrap();
                    SubRange { start, end }
                };
                let Range { start, end } = sub_range.to_absolute(start);
                DiagSpanKind::Range { id, start, end }
            }
            SpanKind::Range { id, start, end } => DiagSpanKind::Range { id, start, end },
        }
    }
}

impl From<Span> for DiagSpan {
    fn from(span: Span) -> Self {
        Self::new_source(span, None)
    }
}

/// A non-empty range targeting a smaller part of a spanned section of text.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct SubRange {
    start: u32,
    end: NonZeroU32,
}

// Convert a usize to a u32 by saturating at `u32::MAX`.
fn to_u32_saturated(value: usize) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
}

impl SubRange {
    /// Create a new sub-range. The given start and end must create a non-empty
    /// range.
    ///
    /// If start or end are above a 2^32-1, they will be saturated.
    pub fn new(start: usize, end: usize) -> Option<Self> {
        if start < end {
            Some(Self {
                start: to_u32_saturated(start),
                // (0 <= start) && (start < end) --> (end != 0)
                end: NonZeroU32::new(to_u32_saturated(end)).unwrap(),
            })
        } else {
            None
        }
    }

    /// Convert to a normal range relative to the spanned range.
    pub fn to_relative(self) -> Range<usize> {
        Range {
            start: self.start as usize,
            end: self.end.get() as usize,
        }
    }

    /// Convert to a normal range at an offset.
    pub fn to_absolute(self, offset: usize) -> Range<usize> {
        Range {
            start: self.start as usize + offset,
            end: self.end.get() as usize + offset,
        }
    }
}

/// A value with a span locating it in the source code.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct Spanned<T> {
    /// The spanned value.
    pub v: T,
    /// The value's location in source code.
    pub span: Span,
}

impl<T> Spanned<T> {
    /// Create a new instance from a value and its span.
    pub const fn new(v: T, span: Span) -> Self {
        Self { v, span }
    }

    /// Create a new instance with a span that does not point into any file.
    pub const fn detached(v: T) -> Self {
        Self { v, span: Span::detached() }
    }

    /// Convert from `&Spanned<T>` to `Spanned<&T>`
    pub const fn as_ref(&self) -> Spanned<&T> {
        Spanned { v: &self.v, span: self.span }
    }

    /// Map the value using a function.
    pub fn map<F, U>(self, f: F) -> Spanned<U>
    where
        F: FnOnce(T) -> U,
    {
        Spanned { v: f(self.v), span: self.span }
    }
}

impl<T: Debug> Debug for Spanned<T> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.v.fmt(f)
    }
}

/// Remaps ranges.
///
/// Useful in combination with
/// [`SyntaxNode::synthesize_mapped`](super::SyntaxNode::synthesize_mapped) to
/// have accurate error spans for source text that is non-consecutive in its
/// source file (for instance, Typst code in a doc comment with start-of-line
/// slashes).
#[derive(Hash)]
pub struct RangeMapper {
    vec: Vec<Mapping>,
    pub(crate) total: usize,
}

/// A mapping from an old index to a new one, guarantees that `old <= new`.
#[derive(Hash, Clone, Copy)]
struct Mapping {
    old: usize,
    new: usize,
}

impl RangeMapper {
    /// Creates a new range mapper.
    ///
    /// The iterator should returns ranges in the original text that will be
    /// consecutively concatenated to produce the derived text.
    ///
    /// Segments should be in order. (The start of a later range must not
    /// precede the end of an earlier range.)
    ///
    /// Note that this representation implies that ranges can only ever increase
    /// in their start position and length when mapped.
    #[track_caller]
    pub fn new(
        segments: impl IntoIterator<Item = Range<usize>>,
    ) -> Result<Self, EcoString> {
        let mut old = 0;
        let mut new = 0;
        let vec = segments
            .into_iter()
            .map(|Range { start, end }| {
                if start > end || new > start {
                    return Err(eco_format!("invalid mapper segment: {start}..{end}"));
                }
                new = start;
                let mapping = Mapping { old, new };
                old += end - new;
                Ok(mapping)
            })
            .collect::<Result<Vec<Mapping>, EcoString>>()?;
        Ok(if vec.is_empty() {
            Self { vec: vec![Mapping { old: 0, new: 0 }], total: 0 }
        } else {
            Self { vec, total: old }
        })
    }

    /// Maps a range in the derived text back to a range in the original text.
    /// If the range spans over multiple segments, the gap between the two
    /// segments will be included in the resulting range.
    ///
    /// Input ranges must have  `start <= end`, and the caller should have
    /// verified that `end <= self.total`.
    pub(crate) fn map(&self, range: Range<usize>) -> Range<usize> {
        debug_assert!(range.start <= range.end);
        if range.end == 0 {
            let offset = self.vec[0].new;
            offset..offset
        } else if range.start == range.end {
            // If start/end are at a boundary, map them to the first position,
            // not the second.
            let offset = self.map_end(range.start);
            offset..offset
        } else {
            let start = self.map_start(range.start);
            // `start >= 0` and `start != end` and `start <= end`, so `end != 0`
            let end = self.map_end(range.end);
            start..end
        }
    }

    /// Map a relative sub-range at an offset to a new sub-range. If the
    /// sub-range spans over multiple segments, the gap between them will be
    /// included in the new sub-range.
    pub(crate) fn map_sub_range(&self, offset: usize, sub_range: SubRange) -> SubRange {
        let range = sub_range.to_absolute(offset);
        let new_offset = self.map_start(offset);
        let start = self.map_start(range.start);
        let end = self.map_end(range.end); // sub-ranges have `start < end`.
        SubRange::new(start - new_offset, end - new_offset).unwrap()
    }

    /// Map a single offset, prefering the second index if at a boundary.
    fn map_start(&self, offset: usize) -> usize {
        let idx = self.vec.partition_point(|&Mapping { old, new: _ }| old <= offset);
        // Subracting by 1 is valid: vec is non-empty, index 0 has `old == 0`,
        // and `partition_point` returns the index of the first item to fail the
        // predicate (or the length), which is not index 0, since `0 <= usize`
        // is true for all usize.
        let Mapping { old, new } = &self.vec[idx - 1];
        new + (offset - old)
    }

    /// Map a single offset, prefering the first index if at a boundary.
    ///
    /// This will panic if the offset is 0.
    fn map_end(&self, offset: usize) -> usize {
        debug_assert_ne!(offset, 0);
        let idx = self.vec.partition_point(|&Mapping { old, new: _ }| old < offset);
        // Unlike `map_start`, this can yield index 0 when `offset == 0`, making
        // `idx - 1` potentially panicking.
        let Mapping { old, new } = &self.vec[idx - 1];
        new + (offset - old)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_detached() {
        let span = Span::detached();
        assert!(span.is_detached());
        assert_eq!(span.id(), None);
    }

    #[test]
    fn test_span_number_encoding() {
        let id = FileId::from_raw(NonZeroU16::new(5).unwrap());
        let span = Span::from_number(id, 10).unwrap();
        assert_eq!(span.id(), Some(id));
        assert_eq!(span.number(), 10);
    }

    #[test]
    fn test_span_range_encoding() {
        let file_id = FileId::from_raw(NonZeroU16::new(u16::MAX).unwrap());
        let roundtrip = |range: Range<usize>| {
            let span = Span::from_range(file_id, range.clone());
            let SpanKind::Range { id, start, end } = span.get() else {
                panic!("bad span kind")
            };
            assert_eq!(id, file_id);
            assert_eq!(start..end, range);
        };

        roundtrip(0..0);
        roundtrip(177..233);
        roundtrip(0..8388607);
        roundtrip(8388606..8388607); // 2^23-2 .. 2^23-1
    }

    #[test]
    fn test_diag_span_range() {
        let file_id = FileId::from_raw(NonZeroU16::new(u16::MAX).unwrap());
        let roundtrip = |range: Range<usize>| {
            let span = DiagSpan::new_range(file_id, range.clone());
            let DiagSpanKind::Range { id, start, end } = span.get() else {
                panic!("bad diagspan kind")
            };
            assert_eq!(id, file_id);
            assert_eq!(start..end, range);
        };

        roundtrip(0..0);
        roundtrip(177..233);
        roundtrip(0..8388607);
        roundtrip(8388606..8388607); // 2^23-2 .. 2^23-1
        roundtrip(8388608..8388609); // 2^23   .. 2^23+1
        #[cfg(target_pointer_width = "64")]
        roundtrip(70368744177662..70368744177663); // 2^46-2 .. 2^46-1
    }

    #[test]
    fn test_sub_range_constructor() {
        let max = u32::MAX as usize;
        // valid
        assert!(SubRange::new(0, 1).is_some());
        assert!(SubRange::new(4, 5).is_some());
        assert!(SubRange::new(0, max).is_some());
        assert!(SubRange::new(0, max - 1).is_some());
        assert!(SubRange::new(max - 1, max).is_some());
        // invalid
        assert!(SubRange::new(0, 0).is_none());
        assert!(SubRange::new(5, 5).is_none());
        assert!(SubRange::new(5, 4).is_none());
        assert!(SubRange::new(max - 1, max - 1).is_none());
        assert!(SubRange::new(max, max).is_none());
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn test_sub_range_saturating() {
        // Values saturate at 2^32-1
        let max = u32::MAX as usize;
        let maxxed = SubRange::new(max, max + 1).unwrap();
        assert_eq!(maxxed.start, maxxed.end.get());
        assert_eq!(SubRange::new(1 << 47, 1 << 63), Some(maxxed));
    }

    #[test]
    fn test_range_mapper() {
        let base = "-- Hello\n-- world\n";
        let ranges = [(3..9), (12..18)];
        let mapped = ranges.iter().map(|r| &base[r.clone()]).collect::<String>();
        let m = RangeMapper::new(ranges).unwrap();

        assert_eq!(mapped, "Hello\nworld\n");
        assert_eq!(m.map(2..3), 5..6); // l -> l
        assert_eq!(m.map(4..6), (7..9)); // o\n -> o\n
        assert_eq!(m.map(6..8), (12..14)); // wo -> wo
        assert_eq!(m.map(8..11), (14..17)); // rld -> rld
        assert_eq!(m.map(2..12), (5..18)); // llo\n-- world\n -> llo\n-- world\n

        // Empty ranges on boundaries:
        assert_eq!(m.map(0..0), (3..3));
        assert_eq!(m.map(6..6), (9..9)); // maps to the left of the boundary
        assert_eq!(m.map(12..12), (18..18));
    }

    /// Small exhaustive edge case tests for the range mapper
    #[test]
    fn test_range_mapper_exhaustive() {
        let empty = RangeMapper::new([]).unwrap();
        assert_eq!(empty.map(0..0), 0..0);

        let exact = RangeMapper::new(Some(0..1)).unwrap();
        assert_eq!(exact.map(0..0), 0..0);
        assert_eq!(exact.map(0..1), 0..1);
        assert_eq!(exact.map(1..1), 1..1);

        let plus = RangeMapper::new(Some(10..11)).unwrap();
        assert_eq!(plus.map(0..0), 10..10);
        assert_eq!(plus.map(0..1), 10..11);
        assert_eq!(plus.map(1..1), 11..11);

        let disjoint = RangeMapper::new([(10..11), (21..22)]).unwrap();
        assert_eq!(disjoint.map(0..0), 10..10);
        assert_eq!(disjoint.map(0..1), 10..11);
        assert_eq!(disjoint.map(0..2), 10..22);
        assert_eq!(disjoint.map(1..1), 11..11);
        assert_eq!(disjoint.map(1..2), 21..22);
        assert_eq!(disjoint.map(2..2), 22..22);

        // disjoint with interspersed empty ranges.
        let with_empty = RangeMapper::new([
            (10..10),
            (10..11),
            (11..11),
            (16..16),
            (21..21),
            (21..22),
            (22..22),
        ])
        .unwrap();
        assert_eq!(with_empty.map(0..0), 10..10);
        assert_eq!(with_empty.map(0..1), 10..11);
        assert_eq!(with_empty.map(0..2), 10..22);
        assert_eq!(with_empty.map(1..1), 11..11);
        assert_eq!(with_empty.map(1..2), 21..22);
        assert_eq!(with_empty.map(2..2), 22..22);
    }

    #[test]
    fn test_sub_range_mapping() {
        let base = "01_23__45";
        let ranges = [(0..2), (3..5), (7..9)];
        let mapped = ranges.iter().map(|r| &base[r.clone()]).collect::<String>();
        assert_eq!(mapped, "012345");
        let m = RangeMapper::new(ranges).unwrap();

        let map_at = |at: usize, sr: Option<SubRange>| {
            let sub_range = sr.unwrap();
            m.map_sub_range(at, sub_range).to_relative()
        };

        // Ranges within each section:
        assert_eq!(map_at(0, SubRange::new(0, 1)), 0..1); // 0
        assert_eq!(map_at(0, SubRange::new(2, 3)), 3..4); // 2
        assert_eq!(map_at(0, SubRange::new(2, 4)), 3..5); // 23
        assert_eq!(map_at(0, SubRange::new(4, 5)), 7..8); // 4
        assert_eq!(map_at(1, SubRange::new(0, 1)), 0..1); // 1
        assert_eq!(map_at(3, SubRange::new(0, 1)), 0..1); // 3
        assert_eq!(map_at(4, SubRange::new(0, 2)), 0..2); // 45
        // Across boundaries:
        assert_eq!(map_at(1, SubRange::new(0, 2)), 0..3); // 12 -> 1_2
        assert_eq!(map_at(0, SubRange::new(1, 3)), 1..4); // 12 -> 1_2
        assert_eq!(map_at(3, SubRange::new(0, 2)), 0..4); // 34 -> 3__4
        assert_eq!(map_at(0, SubRange::new(3, 5)), 4..8); // 34 -> 3__4
        assert_eq!(map_at(1, SubRange::new(0, 4)), 0..7); // 1234 -> 1_23__4
        assert_eq!(map_at(0, SubRange::new(1, 5)), 1..8); // 1234 -> 1_23__4
        assert_eq!(map_at(0, SubRange::new(0, 6)), 0..9); // 012345 -> 01_23__45
    }
}

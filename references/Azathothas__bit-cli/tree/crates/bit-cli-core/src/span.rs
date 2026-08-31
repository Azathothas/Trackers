//! Byte-range algebra over a torrent's linear payload.
//!
//! A torrent is one linear byte stream. Files, pieces, and every scope
//! selector in [`crate::webseed::scope`] all reduce to a set of half-open byte
//! ranges over that stream. Doing the reduction once, here, is what makes the
//! rest of the addressing model tractable:
//!
//! - "which source is responsible for piece N" is a range lookup.
//! - "do the declared sources cover the whole payload" is a subtraction.
//! - "is this request inside the source's scope" is a containment test, which
//!   is asserted at the request layer so an out-of-scope request never reaches
//!   a server as a 416.
//!
//! A [`SpanSet`] is always sorted, non-overlapping, and coalesced, so equality
//! is meaningful and containment is a binary search.

use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// A sorted, non-overlapping, coalesced set of half-open byte ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanSet {
    spans: Vec<Range<u64>>,
}

/// Wire shape of a [`SpanSet`]: explicit `start` and `end` so a caller never
/// has to guess whether the end is inclusive. It is not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanRepr {
    pub start: u64,
    pub end: u64,
}

impl SpanSet {
    /// The empty set.
    pub const fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// A set holding one range. An empty or reversed range yields the empty
    /// set rather than an error, so callers can build from arithmetic without
    /// checking each intermediate.
    pub fn from_range(range: Range<u64>) -> Self {
        if range.start >= range.end {
            return Self::new();
        }
        Self { spans: vec![range] }
    }

    /// Build from any collection of ranges, sorting and coalescing them.
    pub fn from_ranges(ranges: impl IntoIterator<Item = Range<u64>>) -> Self {
        let mut spans: Vec<Range<u64>> = ranges.into_iter().filter(|r| r.start < r.end).collect();
        spans.sort_by_key(|r| (r.start, r.end));
        let mut out: Vec<Range<u64>> = Vec::with_capacity(spans.len());
        for span in spans {
            match out.last_mut() {
                // Touching ranges coalesce as well as overlapping ones, so
                // `0..10` and `10..20` become one span and the set stays
                // canonical.
                Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
                _ => out.push(span),
            }
        }
        Self { spans: out }
    }

    /// The ranges, in order.
    pub fn spans(&self) -> &[Range<u64>] {
        &self.spans
    }

    /// Whether the set holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Total number of bytes covered.
    pub fn len(&self) -> u64 {
        self.spans.iter().map(|r| r.end - r.start).sum()
    }

    /// The lowest and highest byte offsets covered, or `None` when empty.
    pub fn bounds(&self) -> Option<Range<u64>> {
        let first = self.spans.first()?;
        let last = self.spans.last()?;
        Some(first.start..last.end)
    }

    /// Whether `offset` falls inside the set.
    pub fn contains(&self, offset: u64) -> bool {
        self.index_of(offset).is_some()
    }

    /// Whether the whole of `range` falls inside the set.
    ///
    /// This is the check the request layer uses before issuing a ranged GET.
    /// An empty range is trivially contained.
    pub fn contains_range(&self, range: &Range<u64>) -> bool {
        if range.start >= range.end {
            return true;
        }
        match self.index_of(range.start) {
            Some(index) => self.spans[index].end >= range.end,
            None => false,
        }
    }

    /// The contiguous span holding `offset`, if there is one.
    ///
    /// The request layer uses this to clamp a read window: a window may not
    /// run past the end of the span it starts in, because the bytes beyond it
    /// were never bound to this source.
    pub fn span_containing(&self, offset: u64) -> Option<Range<u64>> {
        self.index_of(offset).map(|index| self.spans[index].clone())
    }

    /// Index of the span holding `offset`.
    fn index_of(&self, offset: u64) -> Option<usize> {
        let index = self.spans.partition_point(|r| r.end <= offset);
        let span = self.spans.get(index)?;
        (span.start <= offset).then_some(index)
    }

    /// Everything in either set.
    pub fn union(&self, other: &Self) -> Self {
        Self::from_ranges(self.spans.iter().chain(other.spans.iter()).cloned())
    }

    /// Everything in both sets.
    pub fn intersection(&self, other: &Self) -> Self {
        let mut out = Vec::new();
        let (mut a, mut b) = (0, 0);
        while a < self.spans.len() && b < other.spans.len() {
            let (left, right) = (&self.spans[a], &other.spans[b]);
            let start = left.start.max(right.start);
            let end = left.end.min(right.end);
            if start < end {
                out.push(start..end);
            }
            if left.end < right.end {
                a += 1;
            } else {
                b += 1;
            }
        }
        Self { spans: out }
    }

    /// Everything in this set and not in `other`.
    pub fn difference(&self, other: &Self) -> Self {
        let mut out = Vec::new();
        for span in &self.spans {
            let mut pos = span.start;
            // Only the other set's spans that can overlap this one.
            let first = other.spans.partition_point(|r| r.end <= span.start);
            for cut in &other.spans[first..] {
                if cut.start >= span.end {
                    break;
                }
                if cut.start > pos {
                    out.push(pos..cut.start);
                }
                pos = pos.max(cut.end);
                if pos >= span.end {
                    break;
                }
            }
            if pos < span.end {
                out.push(pos..span.end);
            }
        }
        Self { spans: out }
    }

    /// Clamp every span into `bounds`, dropping anything outside.
    pub fn clamp(&self, bounds: Range<u64>) -> Self {
        self.intersection(&Self::from_range(bounds))
    }

    /// Whether this set covers every byte of `range`.
    pub fn covers(&self, range: Range<u64>) -> bool {
        self.contains_range(&range)
    }

    /// The parts of `range` this set does not cover.
    pub fn gaps_in(&self, range: Range<u64>) -> Self {
        Self::from_range(range).difference(self)
    }

    /// The wire form of the set.
    pub fn to_repr(&self) -> Vec<SpanRepr> {
        self.spans
            .iter()
            .map(|r| SpanRepr {
                start: r.start,
                end: r.end,
            })
            .collect()
    }
}

impl Serialize for SpanSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_repr().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SpanSet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let reprs = Vec::<SpanRepr>::deserialize(deserializer)?;
        Ok(Self::from_ranges(reprs.into_iter().map(|r| r.start..r.end)))
    }
}

impl FromIterator<Range<u64>> for SpanSet {
    fn from_iter<T: IntoIterator<Item = Range<u64>>>(iter: T) -> Self {
        Self::from_ranges(iter)
    }
}

impl fmt::Display for SpanSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.spans.is_empty() {
            return f.write_str("(empty)");
        }
        let text: Vec<String> = self
            .spans
            .iter()
            .map(|r| format!("{}-{}", r.start, r.end - 1))
            .collect();
        f.write_str(&text.join(","))
    }
}

/// Compress a sorted list of indices into inclusive ranges, so a message can
/// say `pieces 10-12, 40` instead of listing forty thousand numbers.
pub fn summarize_indices(indices: &[u32]) -> String {
    if indices.is_empty() {
        return "(none)".to_string();
    }
    let mut parts = Vec::new();
    let mut start = indices[0];
    let mut prev = indices[0];
    for &index in &indices[1..] {
        if index == prev + 1 {
            prev = index;
            continue;
        }
        parts.push(range_text(start, prev));
        start = index;
        prev = index;
    }
    parts.push(range_text(start, prev));
    parts.join(",")
}

fn range_text(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

#[cfg(test)]
mod tests {
    // The assertions here compare a `&[Range<u64>]` against a one-element
    // array of ranges, which is exactly what `spans()` returns. The lint reads
    // that as someone meaning `(0..20).collect()`, which is not what any of
    // these tests are asking.
    #![allow(clippy::single_range_in_vec_init)]

    use super::*;

    fn set(ranges: &[(u64, u64)]) -> SpanSet {
        SpanSet::from_ranges(ranges.iter().map(|&(a, b)| a..b))
    }

    #[test]
    fn overlapping_and_touching_ranges_coalesce() {
        assert_eq!(set(&[(0, 10), (10, 20)]).spans(), &[0..20]);
        assert_eq!(set(&[(0, 15), (10, 20)]).spans(), &[0..20]);
        assert_eq!(set(&[(10, 20), (0, 5)]).spans(), &[0..5, 10..20]);
    }

    #[test]
    fn empty_and_reversed_ranges_are_dropped() {
        assert!(set(&[(5, 5)]).is_empty());
        // Built from variables rather than a literal, because the point of the
        // test is that a reversed range is dropped rather than iterated.
        assert!(set(&[(10, 0)]).is_empty());
    }

    #[test]
    fn length_sums_the_spans() {
        assert_eq!(set(&[(0, 10), (20, 25)]).len(), 15);
        assert_eq!(SpanSet::new().len(), 0);
    }

    #[test]
    fn containment_is_exact_at_the_boundaries() {
        let s = set(&[(10, 20)]);
        assert!(!s.contains(9));
        assert!(s.contains(10));
        assert!(s.contains(19));
        assert!(!s.contains(20));
    }

    #[test]
    fn a_range_must_be_wholly_contained() {
        let s = set(&[(10, 20), (30, 40)]);
        assert!(s.contains_range(&(10..20)));
        assert!(s.contains_range(&(12..18)));
        assert!(!s.contains_range(&(18..22)));
        // A range spanning the gap is not contained even though both ends are.
        assert!(!s.contains_range(&(15..35)));
        assert!(
            s.contains_range(&(5..5)),
            "an empty range is trivially contained"
        );
    }

    #[test]
    fn union_merges_across_sets() {
        assert_eq!(set(&[(0, 10)]).union(&set(&[(10, 20)])).spans(), &[0..20]);
        assert_eq!(
            set(&[(0, 5)]).union(&set(&[(10, 20)])).spans(),
            &[0..5, 10..20]
        );
    }

    #[test]
    fn intersection_keeps_only_shared_bytes() {
        assert_eq!(
            set(&[(0, 20)]).intersection(&set(&[(10, 30)])).spans(),
            &[10..20]
        );
        assert!(set(&[(0, 10)]).intersection(&set(&[(10, 20)])).is_empty());
        assert_eq!(
            set(&[(0, 100)])
                .intersection(&set(&[(10, 20), (30, 40)]))
                .spans(),
            &[10..20, 30..40]
        );
    }

    #[test]
    fn difference_cuts_holes() {
        assert_eq!(
            set(&[(0, 100)]).difference(&set(&[(10, 20)])).spans(),
            &[0..10, 20..100]
        );
        assert!(set(&[(10, 20)]).difference(&set(&[(0, 100)])).is_empty());
        assert_eq!(
            set(&[(0, 100)])
                .difference(&set(&[(0, 10), (90, 100)]))
                .spans(),
            &[10..90]
        );
        assert_eq!(
            set(&[(0, 10)]).difference(&SpanSet::new()).spans(),
            &[0..10]
        );
    }

    #[test]
    fn gaps_name_what_is_uncovered() {
        let covered = set(&[(0, 30), (50, 100)]);
        assert_eq!(covered.gaps_in(0..100).spans(), &[30..50]);
        assert!(covered.gaps_in(0..30).is_empty());
        assert!(set(&[(0, 100)]).covers(0..100));
        assert!(!covered.covers(0..100));
    }

    #[test]
    fn clamping_drops_everything_outside_the_bounds() {
        assert_eq!(set(&[(0, 100)]).clamp(10..20).spans(), &[10..20]);
        assert!(set(&[(200, 300)]).clamp(0..100).is_empty());
    }

    #[test]
    fn display_uses_inclusive_ends_for_people() {
        assert_eq!(set(&[(0, 10), (20, 21)]).to_string(), "0-9,20-20");
        assert_eq!(SpanSet::new().to_string(), "(empty)");
    }

    #[test]
    fn spans_round_trip_through_json() {
        let original = set(&[(0, 10), (20, 30)]);
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, r#"[{"start":0,"end":10},{"start":20,"end":30}]"#);
        let back: SpanSet = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn index_summaries_compress_runs() {
        assert_eq!(summarize_indices(&[]), "(none)");
        assert_eq!(summarize_indices(&[5]), "5");
        assert_eq!(summarize_indices(&[10, 11, 12]), "10-12");
        assert_eq!(summarize_indices(&[1, 2, 3, 7, 9, 10]), "1-3,7,9-10");
    }
}

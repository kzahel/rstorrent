//! Canonical half-open piece-range operations.
//!
//! This module is deterministic, owns no mutable shared state or task, and
//! depends only on the portable `IndexRange` value.

use super::IndexRange;

pub(super) fn insert_range(ranges: &mut Vec<IndexRange>, start: u32, length: u32) {
    if let Some(end) = start.checked_add(length)
        && let Some(range) = IndexRange::new(start, end)
    {
        insert_interval(ranges, range);
    }
}

pub(super) fn insert_interval(ranges: &mut Vec<IndexRange>, mut inserted: IndexRange) {
    let mut output = Vec::with_capacity(ranges.len() + 1);
    let mut placed = false;
    for range in ranges.drain(..) {
        if range.end_exclusive < inserted.start {
            output.push(range);
        } else if inserted.end_exclusive < range.start {
            if !placed {
                output.push(inserted);
                placed = true;
            }
            output.push(range);
        } else {
            inserted.start = inserted.start.min(range.start);
            inserted.end_exclusive = inserted.end_exclusive.max(range.end_exclusive);
        }
    }
    if !placed {
        output.push(inserted);
    }
    *ranges = output;
}

pub(super) fn remove_range(ranges: &mut Vec<IndexRange>, start: u32, length: u32) {
    if let Some(end) = start.checked_add(length)
        && let Some(range) = IndexRange::new(start, end)
    {
        remove_interval(ranges, range);
    }
}

pub(super) fn remove_interval(ranges: &mut Vec<IndexRange>, removed: IndexRange) {
    let mut output = Vec::with_capacity(ranges.len() + 1);
    for range in ranges.drain(..) {
        if range.end_exclusive <= removed.start || range.start >= removed.end_exclusive {
            output.push(range);
            continue;
        }
        if range.start < removed.start {
            output.push(IndexRange {
                start: range.start,
                end_exclusive: removed.start,
            });
        }
        if range.end_exclusive > removed.end_exclusive {
            output.push(IndexRange {
                start: removed.end_exclusive,
                end_exclusive: range.end_exclusive,
            });
        }
    }
    *ranges = output;
}

pub(super) fn difference(left: &[IndexRange], right: &[IndexRange]) -> Vec<IndexRange> {
    let mut output = left.to_vec();
    for range in right {
        remove_interval(&mut output, *range);
    }
    output
}

pub(super) fn range_cardinality(ranges: &[IndexRange]) -> u64 {
    ranges
        .iter()
        .map(|range| u64::from(range.end_exclusive - range.start))
        .sum()
}

pub(crate) fn ranges_from_pieces(pieces: &[bool]) -> Vec<IndexRange> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, present) in pieces
        .iter()
        .copied()
        .chain(std::iter::once(false))
        .enumerate()
    {
        if present && start.is_none() {
            start = Some(index);
        } else if !present && let Some(range_start) = start.take() {
            let Ok(range_start) = u32::try_from(range_start) else {
                break;
            };
            let Ok(end_exclusive) = u32::try_from(index) else {
                break;
            };
            ranges.push(IndexRange {
                start: range_start,
                end_exclusive,
            });
        }
    }
    ranges
}

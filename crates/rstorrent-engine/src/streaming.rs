//! Compact, runtime-independent state for incomplete-file streaming demand.
//!
//! HTTP and other application surfaces express a small number of piece
//! intervals. The engine walks those intervals with bounded cursors rather
//! than expanding them into sets proportional to the number of pieces.

use std::collections::{BTreeMap, BTreeSet};

/// Maximum simultaneous incomplete-file streaming demands for one torrent.
pub const MAX_STREAMING_DEMANDS: usize = 8;

/// Maximum pieces inspected while producing one urgent scheduler batch.
pub const MAX_STREAMING_CANDIDATE_INSPECTIONS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StreamingDemandId(u64);

impl StreamingDemandId {
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingPieceInterval {
    first: u32,
    last: u32,
}

impl StreamingPieceInterval {
    pub fn new(first: u32, last: u32) -> Result<Self, StreamingDemandError> {
        if first > last {
            return Err(StreamingDemandError::InvalidInterval { first, last });
        }
        Ok(Self { first, last })
    }

    #[must_use]
    pub fn first(self) -> u32 {
        self.first
    }

    #[must_use]
    pub fn last(self) -> u32 {
        self.last
    }

    #[must_use]
    pub fn contains(self, piece: u32) -> bool {
        (self.first..=self.last).contains(&piece)
    }

    #[must_use]
    pub fn piece_count(self) -> u64 {
        u64::from(self.last) - u64::from(self.first) + 1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamingUrgency {
    Current,
    Ahead,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamingDemand {
    id: StreamingDemandId,
    current: StreamingPieceInterval,
    ahead: Option<StreamingPieceInterval>,
    admission_order: u64,
    update_generation: u64,
    progress_revision: u64,
}

impl StreamingDemand {
    #[must_use]
    pub fn id(&self) -> StreamingDemandId {
        self.id
    }

    #[must_use]
    pub fn current(&self) -> StreamingPieceInterval {
        self.current
    }

    #[must_use]
    pub fn ahead(&self) -> Option<StreamingPieceInterval> {
        self.ahead
    }

    #[must_use]
    pub fn admission_order(&self) -> u64 {
        self.admission_order
    }

    #[must_use]
    pub fn update_generation(&self) -> u64 {
        self.update_generation
    }

    #[must_use]
    pub fn progress_revision(&self) -> u64 {
        self.progress_revision
    }

    fn contains(&self, piece: u32) -> bool {
        self.current.contains(piece) || self.ahead.is_some_and(|interval| interval.contains(piece))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamingDemandSnapshot {
    revision: u64,
    demands: Vec<StreamingDemand>,
}

impl StreamingDemandSnapshot {
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn demands(&self) -> &[StreamingDemand] {
        &self.demands
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.demands.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamingDemandError {
    InvalidInterval { first: u32, last: u32 },
    Capacity,
    UnknownDemand(StreamingDemandId),
    IdentifierExhausted,
}

/// Mutable compact demand state. Async notification belongs to its owner.
#[derive(Debug, Default)]
pub struct StreamingDemandSet {
    next_id: u64,
    next_order: u64,
    revision: u64,
    demands: BTreeMap<StreamingDemandId, StreamingDemand>,
}

impl StreamingDemandSet {
    #[must_use]
    pub fn snapshot(&self) -> StreamingDemandSnapshot {
        let mut demands = self.demands.values().cloned().collect::<Vec<_>>();
        demands.sort_by_key(|demand| demand.admission_order);
        StreamingDemandSnapshot {
            revision: self.revision,
            demands,
        }
    }

    pub fn insert(
        &mut self,
        current: StreamingPieceInterval,
        ahead: Option<StreamingPieceInterval>,
    ) -> Result<StreamingDemandId, StreamingDemandError> {
        if self.demands.len() == MAX_STREAMING_DEMANDS {
            return Err(StreamingDemandError::Capacity);
        }
        let id_value = self
            .next_id
            .checked_add(1)
            .ok_or(StreamingDemandError::IdentifierExhausted)?;
        let order = self
            .next_order
            .checked_add(1)
            .ok_or(StreamingDemandError::IdentifierExhausted)?;
        self.next_id = id_value;
        self.next_order = order;
        let id = StreamingDemandId(id_value);
        self.demands.insert(
            id,
            StreamingDemand {
                id,
                current,
                ahead,
                admission_order: order,
                update_generation: 0,
                progress_revision: 0,
            },
        );
        self.advance_revision();
        Ok(id)
    }

    pub fn update(
        &mut self,
        id: StreamingDemandId,
        current: StreamingPieceInterval,
        ahead: Option<StreamingPieceInterval>,
    ) -> Result<(), StreamingDemandError> {
        let demand = self
            .demands
            .get_mut(&id)
            .ok_or(StreamingDemandError::UnknownDemand(id))?;
        if demand.current == current && demand.ahead == ahead {
            return Ok(());
        }
        demand.current = current;
        demand.ahead = ahead;
        demand.update_generation = demand.update_generation.saturating_add(1);
        self.advance_revision();
        Ok(())
    }

    pub fn remove(&mut self, id: StreamingDemandId) -> bool {
        let removed = self.demands.remove(&id).is_some();
        if removed {
            self.advance_revision();
        }
        removed
    }

    /// Record useful storage or verification progress for interested demands.
    ///
    /// The work is bounded by [`MAX_STREAMING_DEMANDS`].
    pub fn record_progress(&mut self, piece: u32) -> bool {
        let mut changed = false;
        for demand in self.demands.values_mut() {
            if demand.contains(piece) {
                demand.progress_revision = demand.progress_revision.saturating_add(1);
                changed = true;
            }
        }
        if changed {
            self.advance_revision();
        }
        changed
    }

    fn advance_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamingCandidate {
    pub piece: u32,
    pub urgency: StreamingUrgency,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamingCandidateBatch {
    pub candidates: Vec<StreamingCandidate>,
    pub inspected: usize,
    pub finished: bool,
}

#[derive(Clone, Debug)]
struct DemandCursor {
    current: Option<u32>,
    ahead: Option<u32>,
    demand: StreamingDemand,
}

/// A compact, fair cursor over a demand snapshot.
///
/// At most one cursor per admitted demand is retained. Current intervals are
/// completely considered before any ahead interval, and the inspection budget
/// bounds work even when a 64 KiB HTTP chunk spans 65,536 one-byte pieces.
#[derive(Clone, Debug)]
pub struct StreamingCandidateCursor {
    revision: u64,
    demands: Vec<DemandCursor>,
    current_turn: usize,
    ahead_turn: usize,
    finished: bool,
}

impl StreamingCandidateCursor {
    #[must_use]
    pub fn new(snapshot: &StreamingDemandSnapshot) -> Self {
        let demands = snapshot
            .demands
            .iter()
            .cloned()
            .map(|demand| DemandCursor {
                current: Some(demand.current.first),
                ahead: demand.ahead.map(|interval| interval.first),
                demand,
            })
            .collect();
        Self {
            revision: snapshot.revision,
            demands,
            current_turn: 0,
            ahead_turn: 0,
            finished: snapshot.demands.is_empty(),
        }
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn take<F>(
        &mut self,
        maximum_inspections: usize,
        mut eligible: F,
    ) -> StreamingCandidateBatch
    where
        F: FnMut(u32) -> bool,
    {
        let limit = maximum_inspections.min(MAX_STREAMING_CANDIDATE_INSPECTIONS);
        if limit == 0 || self.finished {
            return StreamingCandidateBatch {
                finished: self.finished,
                ..StreamingCandidateBatch::default()
            };
        }

        let mut candidates = Vec::with_capacity(limit);
        let mut seen = BTreeSet::new();
        let mut inspected = 0;
        while inspected < limit {
            let Some((piece, urgency)) = self.next_piece() else {
                self.finished = true;
                break;
            };
            inspected += 1;
            if eligible(piece) && seen.insert(piece) {
                candidates.push(StreamingCandidate { piece, urgency });
            }
        }
        if self
            .demands
            .iter()
            .all(|cursor| cursor.current.is_none() && cursor.ahead.is_none())
        {
            self.finished = true;
        }
        StreamingCandidateBatch {
            candidates,
            inspected,
            finished: self.finished,
        }
    }

    fn next_piece(&mut self) -> Option<(u32, StreamingUrgency)> {
        if self.demands.is_empty() {
            return None;
        }
        if self.demands.iter().any(|cursor| cursor.current.is_some()) {
            return self
                .next_for_urgency(StreamingUrgency::Current)
                .map(|piece| (piece, StreamingUrgency::Current));
        }
        self.next_for_urgency(StreamingUrgency::Ahead)
            .map(|piece| (piece, StreamingUrgency::Ahead))
    }

    fn next_for_urgency(&mut self, urgency: StreamingUrgency) -> Option<u32> {
        let len = self.demands.len();
        let turn = match urgency {
            StreamingUrgency::Current => &mut self.current_turn,
            StreamingUrgency::Ahead => &mut self.ahead_turn,
        };
        for _ in 0..len {
            let index = *turn % len;
            *turn = (*turn + 1) % len;
            let cursor = &mut self.demands[index];
            let next = match urgency {
                StreamingUrgency::Current => &mut cursor.current,
                StreamingUrgency::Ahead => &mut cursor.ahead,
            };
            let Some(piece) = *next else {
                continue;
            };
            let last = match urgency {
                StreamingUrgency::Current => cursor.demand.current.last,
                StreamingUrgency::Ahead => cursor.demand.ahead.expect("ahead cursor").last,
            };
            *next = (piece < last).then_some(piece + 1);
            return Some(piece);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(first: u32, last: u32) -> StreamingPieceInterval {
        StreamingPieceInterval::new(first, last).unwrap()
    }

    #[test]
    fn interval_is_checked_and_uses_constant_space() {
        assert_eq!(
            StreamingPieceInterval::new(2, 1),
            Err(StreamingDemandError::InvalidInterval { first: 2, last: 1 })
        );
        let pieces = interval(0, 65_535);
        assert_eq!(pieces.piece_count(), 65_536);
        assert!(pieces.contains(42));
        assert_eq!(std::mem::size_of_val(&pieces), 8);
    }

    #[test]
    fn demand_capacity_and_independent_removal_are_bounded() {
        let mut demands = StreamingDemandSet::default();
        let mut ids = Vec::new();
        for piece in 0..MAX_STREAMING_DEMANDS as u32 {
            ids.push(demands.insert(interval(piece, piece), None).unwrap());
        }
        assert_eq!(
            demands.insert(interval(20, 20), None),
            Err(StreamingDemandError::Capacity)
        );
        assert!(demands.remove(ids[3]));
        assert!(!demands.remove(ids[3]));
        assert_eq!(
            demands.snapshot().demands().len(),
            MAX_STREAMING_DEMANDS - 1
        );
        demands.insert(interval(20, 20), None).unwrap();
    }

    #[test]
    fn update_and_progress_revisions_only_advance_when_state_changes() {
        let mut demands = StreamingDemandSet::default();
        let first = demands
            .insert(interval(2, 4), Some(interval(5, 6)))
            .unwrap();
        let second = demands.insert(interval(9, 10), None).unwrap();
        let revision = demands.snapshot().revision();
        demands
            .update(first, interval(2, 4), Some(interval(5, 6)))
            .unwrap();
        assert_eq!(demands.snapshot().revision(), revision);
        assert!(demands.record_progress(3));
        assert!(!demands.record_progress(8));
        let snapshot = demands.snapshot();
        assert_eq!(snapshot.demands()[0].progress_revision(), 1);
        assert_eq!(snapshot.demands()[1].progress_revision(), 0);
        demands.update(second, interval(11, 12), None).unwrap();
        assert_eq!(demands.snapshot().demands()[1].update_generation(), 1);
    }

    #[test]
    fn cursor_is_fair_and_finishes_current_before_ahead() {
        let mut demands = StreamingDemandSet::default();
        demands
            .insert(interval(0, 2), Some(interval(20, 20)))
            .unwrap();
        demands
            .insert(interval(10, 11), Some(interval(30, 30)))
            .unwrap();
        let mut cursor = StreamingCandidateCursor::new(&demands.snapshot());
        let batch = cursor.take(7, |_| true);
        assert_eq!(
            batch.candidates,
            vec![
                StreamingCandidate {
                    piece: 0,
                    urgency: StreamingUrgency::Current
                },
                StreamingCandidate {
                    piece: 10,
                    urgency: StreamingUrgency::Current
                },
                StreamingCandidate {
                    piece: 1,
                    urgency: StreamingUrgency::Current
                },
                StreamingCandidate {
                    piece: 11,
                    urgency: StreamingUrgency::Current
                },
                StreamingCandidate {
                    piece: 2,
                    urgency: StreamingUrgency::Current
                },
                StreamingCandidate {
                    piece: 20,
                    urgency: StreamingUrgency::Ahead
                },
                StreamingCandidate {
                    piece: 30,
                    urgency: StreamingUrgency::Ahead
                },
            ]
        );
        assert!(batch.finished);
    }

    #[test]
    fn cursor_deduplicates_overlap_and_bounds_tiny_piece_work() {
        let mut demands = StreamingDemandSet::default();
        demands.insert(interval(0, 65_535), None).unwrap();
        demands.insert(interval(0, 65_535), None).unwrap();
        let snapshot = demands.snapshot();
        let mut cursor = StreamingCandidateCursor::new(&snapshot);
        let batch = cursor.take(usize::MAX, |piece| piece % 2 == 0);
        assert_eq!(batch.inspected, MAX_STREAMING_CANDIDATE_INSPECTIONS);
        assert_eq!(
            batch.candidates.len(),
            MAX_STREAMING_CANDIDATE_INSPECTIONS / 4
        );
        assert!(!batch.finished);
        assert_eq!(cursor.demands.len(), 2);
    }

    #[test]
    fn cursor_skips_ineligible_pieces_without_exceeding_budget() {
        let mut demands = StreamingDemandSet::default();
        demands.insert(interval(0, 999), None).unwrap();
        let mut cursor = StreamingCandidateCursor::new(&demands.snapshot());
        let batch = cursor.take(17, |piece| piece == 16);
        assert_eq!(batch.inspected, 17);
        assert_eq!(
            batch.candidates.as_slice(),
            &[StreamingCandidate {
                piece: 16,
                urgency: StreamingUrgency::Current,
            }]
        );
    }
}

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DurabilityTarget {
    WantedFile(usize),
    PartFile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointIntent {
    pub(crate) piece_index: usize,
    pub(crate) length: u64,
    pub(crate) verified_at: Duration,
    pub(crate) targets: Vec<DurabilityTarget>,
}

impl CheckpointIntent {
    pub(crate) fn new(
        piece_index: usize,
        length: u64,
        verified_at: Duration,
        targets: impl IntoIterator<Item = DurabilityTarget>,
    ) -> Result<Self, CheckpointStateError> {
        if length == 0 {
            return Err(CheckpointStateError::ZeroLengthPiece);
        }
        let mut targets = targets.into_iter().collect::<Vec<_>>();
        targets.sort_unstable();
        targets.dedup();
        if targets.is_empty() {
            return Err(CheckpointStateError::NoDurabilityTarget);
        }
        Ok(Self {
            piece_index,
            length,
            verified_at,
            targets,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointPolicy {
    max_age: Duration,
    max_dirty_bytes: u64,
    max_pieces: usize,
}

impl CheckpointPolicy {
    pub(crate) fn new(
        max_age: Duration,
        max_dirty_bytes: u64,
        max_pieces: usize,
    ) -> Result<Self, CheckpointStateError> {
        if max_age.is_zero() {
            return Err(CheckpointStateError::ZeroMaximumAge);
        }
        if max_dirty_bytes == 0 {
            return Err(CheckpointStateError::ZeroMaximumDirtyBytes);
        }
        if max_pieces == 0 {
            return Err(CheckpointStateError::ZeroMaximumPieces);
        }
        Ok(Self {
            max_age,
            max_dirty_bytes,
            max_pieces,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CheckpointTrigger {
    MaximumAge,
    MaximumDirtyBytes,
    MaximumPieces,
    OversizedPiece,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CheckpointAdmission {
    Accumulating,
    Ready(CheckpointTrigger),
    FlushBefore {
        trigger: CheckpointTrigger,
        intent: CheckpointIntent,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointBatch {
    pub(crate) intents: Vec<CheckpointIntent>,
    pub(crate) dirty_bytes: u64,
    pub(crate) oldest_verified_at: Duration,
    pub(crate) targets: Vec<DurabilityTarget>,
}

#[derive(Clone, Debug)]
pub(crate) struct CheckpointBatchState {
    policy: CheckpointPolicy,
    intents: Vec<CheckpointIntent>,
    piece_indices: BTreeSet<usize>,
    dirty_bytes: u64,
    oldest_verified_at: Option<Duration>,
    targets: BTreeSet<DurabilityTarget>,
}

impl CheckpointBatchState {
    pub(crate) fn new(policy: CheckpointPolicy) -> Self {
        Self {
            policy,
            intents: Vec::new(),
            piece_indices: BTreeSet::new(),
            dirty_bytes: 0,
            oldest_verified_at: None,
            targets: BTreeSet::new(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.intents.len()
    }

    pub(crate) fn dirty_bytes(&self) -> u64 {
        self.dirty_bytes
    }

    pub(crate) fn oldest_age(&self, now: Duration) -> Option<Duration> {
        self.oldest_verified_at
            .map(|oldest| now.saturating_sub(oldest))
    }

    pub(crate) fn flush_reason(&self, now: Duration) -> Option<CheckpointTrigger> {
        (!self.intents.is_empty()
            && self
                .oldest_age(now)
                .is_some_and(|age| age >= self.policy.max_age))
        .then_some(CheckpointTrigger::MaximumAge)
    }

    pub(crate) fn next_flush_in(&self, now: Duration) -> Option<Duration> {
        self.oldest_age(now)
            .map(|age| self.policy.max_age.saturating_sub(age))
    }

    pub(crate) fn admit(
        &mut self,
        intent: CheckpointIntent,
        now: Duration,
    ) -> Result<CheckpointAdmission, CheckpointStateError> {
        if self.piece_indices.contains(&intent.piece_index) {
            return Err(CheckpointStateError::DuplicatePiece(intent.piece_index));
        }
        if let Some(trigger) = self.flush_reason(now) {
            return Ok(CheckpointAdmission::FlushBefore { trigger, intent });
        }
        let next_dirty_bytes = self
            .dirty_bytes
            .checked_add(intent.length)
            .ok_or(CheckpointStateError::DirtyBytesOverflow)?;
        if !self.intents.is_empty() {
            if self.intents.len() >= self.policy.max_pieces {
                return Ok(CheckpointAdmission::FlushBefore {
                    trigger: CheckpointTrigger::MaximumPieces,
                    intent,
                });
            }
            if next_dirty_bytes > self.policy.max_dirty_bytes {
                return Ok(CheckpointAdmission::FlushBefore {
                    trigger: CheckpointTrigger::MaximumDirtyBytes,
                    intent,
                });
            }
        }

        let piece_index = intent.piece_index;
        let length = intent.length;
        let verified_at = intent.verified_at;
        for &target in &intent.targets {
            self.targets.insert(target);
        }
        self.piece_indices.insert(piece_index);
        self.intents.push(intent);
        self.dirty_bytes = next_dirty_bytes;
        self.oldest_verified_at = Some(
            self.oldest_verified_at
                .map_or(verified_at, |oldest| oldest.min(verified_at)),
        );

        if length > self.policy.max_dirty_bytes && self.intents.len() == 1 {
            return Ok(CheckpointAdmission::Ready(
                CheckpointTrigger::OversizedPiece,
            ));
        }
        if self.dirty_bytes >= self.policy.max_dirty_bytes {
            return Ok(CheckpointAdmission::Ready(
                CheckpointTrigger::MaximumDirtyBytes,
            ));
        }
        if self.intents.len() >= self.policy.max_pieces {
            return Ok(CheckpointAdmission::Ready(CheckpointTrigger::MaximumPieces));
        }
        Ok(CheckpointAdmission::Accumulating)
    }

    pub(crate) fn take(&mut self) -> Option<CheckpointBatch> {
        let oldest_verified_at = self.oldest_verified_at.take()?;
        let batch = CheckpointBatch {
            intents: std::mem::take(&mut self.intents),
            dirty_bytes: std::mem::take(&mut self.dirty_bytes),
            oldest_verified_at,
            targets: std::mem::take(&mut self.targets).into_iter().collect(),
        };
        self.piece_indices.clear();
        Some(batch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CheckpointStateError {
    ZeroMaximumAge,
    ZeroMaximumDirtyBytes,
    ZeroMaximumPieces,
    ZeroLengthPiece,
    NoDurabilityTarget,
    DuplicatePiece(usize),
    DirtyBytesOverflow,
}

impl fmt::Display for CheckpointStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroMaximumAge => formatter.write_str("checkpoint maximum age is zero"),
            Self::ZeroMaximumDirtyBytes => {
                formatter.write_str("checkpoint maximum dirty bytes is zero")
            }
            Self::ZeroMaximumPieces => formatter.write_str("checkpoint maximum pieces is zero"),
            Self::ZeroLengthPiece => formatter.write_str("checkpoint piece length is zero"),
            Self::NoDurabilityTarget => {
                formatter.write_str("checkpoint piece has no durability target")
            }
            Self::DuplicatePiece(piece) => {
                write!(formatter, "checkpoint piece {piece} is already dirty")
            }
            Self::DirtyBytesOverflow => formatter.write_str("checkpoint dirty bytes overflow"),
        }
    }
}

impl Error for CheckpointStateError {}

#[cfg(test)]
mod tests {
    use super::{
        CheckpointAdmission, CheckpointBatchState, CheckpointIntent, CheckpointPolicy,
        CheckpointStateError, CheckpointTrigger, DurabilityTarget,
    };
    use std::time::Duration;

    fn policy(bytes: u64, pieces: usize) -> CheckpointPolicy {
        CheckpointPolicy::new(Duration::from_secs(2), bytes, pieces).expect("policy")
    }

    fn intent(piece: usize, length: u64, seconds: u64) -> CheckpointIntent {
        CheckpointIntent::new(
            piece,
            length,
            Duration::from_secs(seconds),
            [
                DurabilityTarget::WantedFile(piece % 2),
                DurabilityTarget::PartFile,
                DurabilityTarget::WantedFile(piece % 2),
            ],
        )
        .expect("intent")
    }

    #[test]
    fn validates_policy_and_intent_bounds() {
        assert_eq!(
            CheckpointPolicy::new(Duration::ZERO, 1, 1),
            Err(CheckpointStateError::ZeroMaximumAge)
        );
        assert_eq!(
            CheckpointPolicy::new(Duration::from_secs(1), 0, 1),
            Err(CheckpointStateError::ZeroMaximumDirtyBytes)
        );
        assert_eq!(
            CheckpointPolicy::new(Duration::from_secs(1), 1, 0),
            Err(CheckpointStateError::ZeroMaximumPieces)
        );
        assert_eq!(
            CheckpointIntent::new(0, 0, Duration::ZERO, [DurabilityTarget::PartFile]),
            Err(CheckpointStateError::ZeroLengthPiece)
        );
        assert_eq!(
            CheckpointIntent::new(0, 1, Duration::ZERO, []),
            Err(CheckpointStateError::NoDurabilityTarget)
        );
    }

    #[test]
    fn exact_byte_limit_flushes_one_deduplicated_batch() {
        let mut state = CheckpointBatchState::new(policy(8, 8));
        assert_eq!(
            state.admit(intent(0, 4, 1), Duration::from_secs(1)),
            Ok(CheckpointAdmission::Accumulating)
        );
        assert_eq!(
            state.admit(intent(1, 4, 1), Duration::from_secs(1)),
            Ok(CheckpointAdmission::Ready(
                CheckpointTrigger::MaximumDirtyBytes
            ))
        );
        let batch = state.take().expect("batch");
        assert_eq!(batch.intents.len(), 2);
        assert_eq!(batch.dirty_bytes, 8);
        assert_eq!(batch.oldest_verified_at, Duration::from_secs(1));
        assert_eq!(
            batch.targets,
            vec![
                DurabilityTarget::WantedFile(0),
                DurabilityTarget::WantedFile(1),
                DurabilityTarget::PartFile,
            ]
        );
        assert_eq!(state.len(), 0);
        assert_eq!(state.dirty_bytes(), 0);
    }

    #[test]
    fn byte_overflow_flushes_current_before_returned_intent() {
        let mut state = CheckpointBatchState::new(policy(8, 8));
        state
            .admit(intent(0, 5, 1), Duration::from_secs(1))
            .expect("first intent");
        let next = intent(1, 4, 1);
        assert_eq!(
            state.admit(next.clone(), Duration::from_secs(1)),
            Ok(CheckpointAdmission::FlushBefore {
                trigger: CheckpointTrigger::MaximumDirtyBytes,
                intent: next,
            })
        );
        assert_eq!(state.len(), 1);
        assert_eq!(state.dirty_bytes(), 5);
    }

    #[test]
    fn exact_piece_limit_and_duplicate_piece_are_bounded() {
        let mut state = CheckpointBatchState::new(policy(100, 2));
        state
            .admit(intent(0, 1, 1), Duration::from_secs(1))
            .expect("first intent");
        assert_eq!(
            state.admit(intent(1, 1, 1), Duration::from_secs(1)),
            Ok(CheckpointAdmission::Ready(CheckpointTrigger::MaximumPieces))
        );
        assert_eq!(
            state.admit(intent(1, 1, 1), Duration::from_secs(1)),
            Err(CheckpointStateError::DuplicatePiece(1))
        );
        assert_eq!(state.len(), 2);
    }

    #[test]
    fn maximum_age_flushes_before_admitting_newer_work() {
        let mut state = CheckpointBatchState::new(policy(100, 8));
        state
            .admit(intent(0, 1, 10), Duration::from_secs(10))
            .expect("first intent");
        assert_eq!(state.flush_reason(Duration::from_millis(11_999)), None);
        assert_eq!(
            state.flush_reason(Duration::from_secs(12)),
            Some(CheckpointTrigger::MaximumAge)
        );
        let next = intent(1, 1, 12);
        assert_eq!(
            state.admit(next.clone(), Duration::from_secs(12)),
            Ok(CheckpointAdmission::FlushBefore {
                trigger: CheckpointTrigger::MaximumAge,
                intent: next,
            })
        );
        assert_eq!(
            state.oldest_age(Duration::from_secs(9)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn one_oversized_piece_is_admitted_alone_for_liveness() {
        let mut state = CheckpointBatchState::new(policy(8, 8));
        assert_eq!(
            state.admit(intent(0, 9, 1), Duration::from_secs(1)),
            Ok(CheckpointAdmission::Ready(
                CheckpointTrigger::OversizedPiece
            ))
        );
        assert_eq!(state.len(), 1);
        assert_eq!(state.dirty_bytes(), 9);

        let next = intent(1, 1, 1);
        assert_eq!(
            state.admit(next.clone(), Duration::from_secs(1)),
            Ok(CheckpointAdmission::FlushBefore {
                trigger: CheckpointTrigger::MaximumDirtyBytes,
                intent: next,
            })
        );
    }
}

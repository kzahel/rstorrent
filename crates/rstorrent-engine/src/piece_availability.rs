//! Compact task-free authority for locally verified and readable pieces.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use rstorrent_protocol::peer_wire::PeerMessage;

pub const MAX_LOCAL_AVAILABILITY_PIECES: usize = 2_097_152;
pub const MAX_LOCAL_AVAILABILITY_BYTES: usize = MAX_LOCAL_AVAILABILITY_PIECES / 8;
pub const MAX_AVAILABILITY_CHANGES: usize = 4_096;
pub const MAX_AVAILABILITY_DRAIN: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailabilityCursor {
    pub epoch: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilitySnapshot {
    pub epoch: u64,
    pub revision: u64,
    pub piece_count: usize,
    pub available_count: usize,
    bits: Arc<[u8]>,
}

impl AvailabilitySnapshot {
    pub fn bitfield(&self) -> &[u8] {
        &self.bits
    }

    pub fn is_available(&self, piece: usize) -> bool {
        piece < self.piece_count && bit_is_set(&self.bits, piece)
    }

    pub fn initial_message(&self, fast: bool) -> Option<PeerMessage> {
        if fast && self.available_count == self.piece_count {
            Some(PeerMessage::HaveAll)
        } else if fast && self.available_count == 0 {
            Some(PeerMessage::HaveNone)
        } else if self.available_count == 0 {
            None
        } else {
            Some(PeerMessage::Bitfield(self.bits.to_vec()))
        }
    }

    pub const fn cursor(&self) -> AvailabilityCursor {
        AvailabilityCursor {
            epoch: self.epoch,
            revision: self.revision,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AvailabilityDrain {
    Changes {
        cursor: AvailabilityCursor,
        pieces: Vec<u32>,
        more: bool,
    },
    EpochChanged(AvailabilitySnapshot),
    Lagged,
}

#[derive(Clone, Debug)]
pub struct PieceAvailability {
    inner: Arc<Mutex<AvailabilityState>>,
}

#[derive(Debug)]
struct AvailabilityState {
    epoch: u64,
    revision: u64,
    piece_count: usize,
    available_count: usize,
    bits: Vec<u8>,
    changes: VecDeque<AvailabilityChange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AvailabilityChange {
    revision: u64,
    piece: u32,
}

impl PieceAvailability {
    pub fn new(epoch: u64, available: &[bool]) -> Result<Self, &'static str> {
        validate_piece_count(available.len())?;
        let mut bits = vec![0_u8; available.len().div_ceil(8)];
        let mut available_count = 0;
        for (piece, value) in available.iter().copied().enumerate() {
            if value {
                set_bit(&mut bits, piece, true);
                available_count += 1;
            }
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(AvailabilityState {
                epoch,
                revision: 0,
                piece_count: available.len(),
                available_count,
                bits,
                changes: VecDeque::new(),
            })),
        })
    }

    pub fn empty(piece_count: usize, epoch: u64) -> Result<Self, &'static str> {
        validate_piece_count(piece_count)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(AvailabilityState {
                epoch,
                revision: 0,
                piece_count,
                available_count: 0,
                bits: vec![0; piece_count.div_ceil(8)],
                changes: VecDeque::new(),
            })),
        })
    }

    pub fn snapshot(&self) -> AvailabilitySnapshot {
        let state = self.state();
        AvailabilitySnapshot {
            epoch: state.epoch,
            revision: state.revision,
            piece_count: state.piece_count,
            available_count: state.available_count,
            bits: state.bits.clone().into(),
        }
    }

    pub fn is_available(&self, piece: usize, epoch: u64) -> bool {
        let state = self.state();
        state.epoch == epoch && piece < state.piece_count && bit_is_set(&state.bits, piece)
    }

    pub fn publish(&self, piece: usize, epoch: u64) -> Result<bool, &'static str> {
        let mut state = self.state();
        if state.epoch != epoch {
            return Err("availability storage epoch is stale");
        }
        if piece >= state.piece_count {
            return Err("availability piece is outside torrent geometry");
        }
        if bit_is_set(&state.bits, piece) {
            return Ok(false);
        }
        let revision = next_revision(state.revision)?;
        let piece = u32::try_from(piece).map_err(|_| "availability piece index overflow")?;
        set_bit(&mut state.bits, piece as usize, true);
        state.available_count += 1;
        state.revision = revision;
        state
            .changes
            .push_back(AvailabilityChange { revision, piece });
        if state.changes.len() > MAX_AVAILABILITY_CHANGES {
            state.changes.pop_front();
        }
        Ok(true)
    }

    pub fn replace_epoch(&self, epoch: u64, available: &[bool]) -> Result<(), &'static str> {
        validate_piece_count(available.len())?;
        let mut state = self.state();
        if available.len() != state.piece_count {
            return Err("replacement availability geometry changed");
        }
        if epoch <= state.epoch {
            return Err("replacement availability epoch did not advance");
        }
        let mut bits = vec![0_u8; available.len().div_ceil(8)];
        let mut available_count = 0;
        for (piece, value) in available.iter().copied().enumerate() {
            if value {
                set_bit(&mut bits, piece, true);
                available_count += 1;
            }
        }
        state.epoch = epoch;
        state.revision = next_revision(state.revision)?;
        state.available_count = available_count;
        state.bits = bits;
        state.changes.clear();
        Ok(())
    }

    pub fn invalidate_epoch(&self, epoch: u64) -> Result<bool, &'static str> {
        let mut state = self.state();
        if state.epoch != epoch {
            return Ok(false);
        }
        state.epoch = state
            .epoch
            .checked_add(1)
            .ok_or("availability storage epoch overflow")?;
        state.revision = next_revision(state.revision)?;
        state.available_count = 0;
        state.bits.fill(0);
        state.changes.clear();
        Ok(true)
    }

    pub fn drain(&self, cursor: AvailabilityCursor) -> AvailabilityDrain {
        let state = self.state();
        if cursor.epoch != state.epoch {
            return AvailabilityDrain::EpochChanged(AvailabilitySnapshot {
                epoch: state.epoch,
                revision: state.revision,
                piece_count: state.piece_count,
                available_count: state.available_count,
                bits: state.bits.clone().into(),
            });
        }
        if cursor.revision > state.revision {
            return AvailabilityDrain::Lagged;
        }
        let oldest_retained = state
            .changes
            .front()
            .map_or(state.revision.saturating_add(1), |change| change.revision);
        if cursor.revision.saturating_add(1) < oldest_retained {
            return AvailabilityDrain::Lagged;
        }
        let pieces = state
            .changes
            .iter()
            .filter(|change| change.revision > cursor.revision)
            .take(MAX_AVAILABILITY_DRAIN)
            .map(|change| change.piece)
            .collect::<Vec<_>>();
        let revision = state
            .changes
            .iter()
            .filter(|change| change.revision > cursor.revision)
            .take(MAX_AVAILABILITY_DRAIN)
            .last()
            .map_or(cursor.revision, |change| change.revision);
        AvailabilityDrain::Changes {
            cursor: AvailabilityCursor {
                epoch: state.epoch,
                revision,
            },
            pieces,
            more: revision < state.revision,
        }
    }

    fn state(&self) -> MutexGuard<'_, AvailabilityState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn validate_piece_count(piece_count: usize) -> Result<(), &'static str> {
    if piece_count == 0 || piece_count > MAX_LOCAL_AVAILABILITY_PIECES {
        return Err("availability piece count is outside supported geometry");
    }
    Ok(())
}

fn next_revision(revision: u64) -> Result<u64, &'static str> {
    revision
        .checked_add(1)
        .ok_or("availability revision overflow")
}

fn bit_is_set(bits: &[u8], piece: usize) -> bool {
    bits.get(piece / 8)
        .is_some_and(|byte| byte & (1 << (7 - piece % 8)) != 0)
}

fn set_bit(bits: &mut [u8], piece: usize, value: bool) {
    let mask = 1 << (7 - piece % 8);
    if value {
        bits[piece / 8] |= mask;
    } else {
        bits[piece / 8] &= !mask;
    }
}

#[cfg(test)]
mod tests {
    use rstorrent_protocol::peer_wire::PeerMessage;

    use super::{
        AvailabilityCursor, AvailabilityDrain, MAX_AVAILABILITY_CHANGES, MAX_AVAILABILITY_DRAIN,
        MAX_LOCAL_AVAILABILITY_PIECES, PieceAvailability,
    };

    #[test]
    fn compact_initial_forms_are_exact_and_spare_bits_are_zero() {
        let none = PieceAvailability::new(7, &[false; 10]).expect("none");
        let all = PieceAvailability::new(7, &[true; 10]).expect("all");
        let mixed = PieceAvailability::new(
            7,
            &[
                true, false, true, true, false, false, false, true, true, false,
            ],
        )
        .expect("mixed");
        assert_eq!(none.snapshot().initial_message(false), None);
        assert_eq!(
            none.snapshot().initial_message(true),
            Some(PeerMessage::HaveNone)
        );
        assert_eq!(
            all.snapshot().initial_message(true),
            Some(PeerMessage::HaveAll)
        );
        assert_eq!(
            mixed.snapshot().initial_message(false),
            Some(PeerMessage::Bitfield(vec![0b1011_0001, 0b1000_0000]))
        );
    }

    #[test]
    fn publishes_once_and_drains_sixteen_changes_per_cursor_step() {
        let availability = PieceAvailability::empty(32, 3).expect("availability");
        let initial = availability.snapshot().cursor();
        for piece in 0..20 {
            assert!(availability.publish(piece, 3).expect("publish"));
            assert!(!availability.publish(piece, 3).expect("duplicate"));
        }
        let AvailabilityDrain::Changes {
            cursor,
            pieces,
            more,
        } = availability.drain(initial)
        else {
            panic!("expected changes");
        };
        assert_eq!(pieces.len(), MAX_AVAILABILITY_DRAIN);
        assert!(more);
        let AvailabilityDrain::Changes { pieces, more, .. } = availability.drain(cursor) else {
            panic!("expected remainder");
        };
        assert_eq!(pieces, [16, 17, 18, 19]);
        assert!(!more);
    }

    #[test]
    fn epoch_replacement_forces_a_fresh_snapshot() {
        let availability = PieceAvailability::new(1, &[true, false]).expect("availability");
        let cursor = availability.snapshot().cursor();
        availability
            .replace_epoch(2, &[false, true])
            .expect("replace");
        let AvailabilityDrain::EpochChanged(snapshot) = availability.drain(cursor) else {
            panic!("expected epoch replacement");
        };
        assert_eq!(snapshot.epoch, 2);
        assert!(!snapshot.is_available(0));
        assert!(snapshot.is_available(1));
    }

    #[test]
    fn invalidation_withdraws_every_piece_and_fences_the_old_epoch() {
        let availability = PieceAvailability::new(4, &[true, false, true]).expect("availability");
        let cursor = availability.snapshot().cursor();
        assert!(availability.invalidate_epoch(4).expect("invalidate"));
        assert!(!availability.invalidate_epoch(4).expect("stale invalidate"));
        let AvailabilityDrain::EpochChanged(snapshot) = availability.drain(cursor) else {
            panic!("expected invalidated epoch");
        };
        assert_eq!(snapshot.epoch, 5);
        assert_eq!(snapshot.available_count, 0);
        assert_eq!(snapshot.bitfield(), &[0]);
        assert!(!availability.is_available(0, 4));
    }

    #[test]
    fn bounded_timeline_closes_a_lagging_cursor() {
        let availability =
            PieceAvailability::empty(MAX_AVAILABILITY_CHANGES + 1, 9).expect("availability");
        for piece in 0..=MAX_AVAILABILITY_CHANGES {
            availability.publish(piece, 9).expect("publish");
        }
        assert_eq!(
            availability.drain(AvailabilityCursor {
                epoch: 9,
                revision: 0,
            }),
            AvailabilityDrain::Lagged
        );
    }

    #[test]
    fn maximum_geometry_stays_within_the_compact_byte_bound() {
        let availability = PieceAvailability::empty(MAX_LOCAL_AVAILABILITY_PIECES, 1)
            .expect("maximum availability");
        assert_eq!(availability.snapshot().bitfield().len(), 262_144);
    }
}

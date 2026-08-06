//! Compact runtime-independent policy and availability index for piece activation.

use std::cmp::Ordering;

pub(crate) const MAX_PICKER_CANDIDATE_INSPECTIONS: usize = 256;

const POSITION_UNWANTED: u32 = u32::MAX;
const POSITION_PLANNED: u32 = u32::MAX - 1;
const POSITION_RESERVED: u32 = u32::MAX - 2;
const POSITION_DETACHED: u32 = u32::MAX - 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PieceActivationPolicy {
    InOrder,
    RarestFirst,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PiecePickerCounters {
    pub rank_comparisons: u64,
    pub single_piece_updates: u64,
    pub bulk_rebuilds: u64,
    pub candidate_inspections: u64,
}

#[derive(Debug)]
pub(crate) struct AvailabilityPicker {
    policy: PieceActivationPolicy,
    tie_seed: u64,
    counts: Vec<u16>,
    heap: Vec<u32>,
    positions: Vec<u32>,
    ranked_len: usize,
    eligibility_changed: bool,
    seed_count: u16,
    wanted_remaining: usize,
    counters: PiecePickerCounters,
}

impl AvailabilityPicker {
    pub(crate) fn new(
        piece_count: usize,
        wanted: Vec<u32>,
        policy: PieceActivationPolicy,
        tie_seed: u64,
    ) -> Result<Self, &'static str> {
        if piece_count == 0 || piece_count > u32::MAX as usize {
            return Err("piece picker geometry is invalid");
        }
        let mut positions = vec![POSITION_UNWANTED; piece_count];
        for (position, &piece) in wanted.iter().enumerate() {
            let index = piece as usize;
            if index >= piece_count {
                return Err("wanted piece is outside picker geometry");
            }
            if positions[index] != POSITION_UNWANTED {
                return Err("wanted piece is duplicated");
            }
            positions[index] = position as u32;
        }
        let wanted_remaining = wanted.len();
        let ranked_len = wanted.len();
        let mut picker = Self {
            policy,
            tie_seed,
            counts: vec![0; piece_count],
            heap: wanted,
            positions,
            ranked_len,
            eligibility_changed: false,
            seed_count: 0,
            wanted_remaining,
            counters: PiecePickerCounters::default(),
        };
        picker.rebuild_heap(false);
        Ok(picker)
    }

    pub(crate) const fn policy(&self) -> PieceActivationPolicy {
        self.policy
    }

    pub(crate) const fn seed_count(&self) -> u16 {
        self.seed_count
    }

    pub(crate) const fn wanted_remaining(&self) -> usize {
        self.wanted_remaining
    }

    pub(crate) fn is_wanted(&self, piece: usize) -> bool {
        self.positions
            .get(piece)
            .is_some_and(|position| *position != POSITION_UNWANTED)
    }

    pub(crate) fn availability(&self, piece: usize) -> Option<u32> {
        self.counts
            .get(piece)
            .map(|count| u32::from(*count) + u32::from(self.seed_count))
    }

    pub(crate) fn increment_piece(&mut self, piece: usize) -> Result<(), &'static str> {
        self.increment_piece_without_repair(piece)?;
        self.counters.single_piece_updates = self.counters.single_piece_updates.saturating_add(1);
        self.repair_piece(piece);
        self.note_eligibility_change();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn decrement_piece(&mut self, piece: usize) -> Result<(), &'static str> {
        self.decrement_piece_without_repair(piece)?;
        self.counters.single_piece_updates = self.counters.single_piece_updates.saturating_add(1);
        self.repair_piece(piece);
        self.note_eligibility_change();
        Ok(())
    }

    pub(crate) fn increment_piece_without_repair(
        &mut self,
        piece: usize,
    ) -> Result<(), &'static str> {
        let count = self
            .counts
            .get_mut(piece)
            .ok_or("availability piece is outside picker geometry")?;
        *count = count
            .checked_add(1)
            .ok_or("availability counter overflow")?;
        Ok(())
    }

    pub(crate) fn decrement_piece_without_repair(
        &mut self,
        piece: usize,
    ) -> Result<(), &'static str> {
        let count = self
            .counts
            .get_mut(piece)
            .ok_or("availability piece is outside picker geometry")?;
        *count = count
            .checked_sub(1)
            .ok_or("availability counter underflow")?;
        Ok(())
    }

    pub(crate) fn increment_seed_without_rebuild(&mut self) -> Result<(), &'static str> {
        self.seed_count = self
            .seed_count
            .checked_add(1)
            .ok_or("seed availability counter overflow")?;
        Ok(())
    }

    pub(crate) fn decrement_seed_without_rebuild(&mut self) -> Result<(), &'static str> {
        self.seed_count = self
            .seed_count
            .checked_sub(1)
            .ok_or("seed availability counter underflow")?;
        Ok(())
    }

    pub(crate) fn rebuild_after_bulk_update(&mut self) {
        self.rebuild_heap(true);
    }

    pub(crate) fn note_eligibility_change(&mut self) {
        self.eligibility_changed |= self.ranked_len != self.heap.len();
    }

    pub(crate) fn precedes(&self, lhs: usize, rhs: usize) -> bool {
        self.rank(lhs, rhs) == Ordering::Less
    }

    pub(crate) fn reserve_best_matching(
        &mut self,
        mut eligible: impl FnMut(usize) -> bool,
    ) -> Option<u32> {
        if self.ranked_len == 0 && self.eligibility_changed {
            self.rebuild_heap(false);
        }
        for _ in 0..MAX_PICKER_CANDIDATE_INSPECTIONS {
            if self.ranked_len == 0 {
                break;
            }
            let piece = self.heap[0];
            if self.availability(piece as usize) == Some(0) {
                break;
            }
            self.counters.candidate_inspections =
                self.counters.candidate_inspections.saturating_add(1);
            if eligible(piece as usize) {
                return Some(self.remove_ranked_root(POSITION_RESERVED));
            }
            self.defer_ranked_root();
        }
        None
    }

    pub(crate) fn cancel_reserved(&mut self, piece: usize) -> Result<(), &'static str> {
        if self.positions.get(piece) != Some(&POSITION_RESERVED) {
            return Err("piece is not reserved for planning");
        }
        self.positions[piece] = POSITION_DETACHED;
        self.push_detached(piece as u32);
        Ok(())
    }

    pub(crate) fn reserve_specific(&mut self, piece: usize) -> bool {
        let Some(&position) = self.positions.get(piece) else {
            return false;
        };
        if position >= POSITION_DETACHED || self.availability(piece) == Some(0) {
            return false;
        }
        self.remove_at(position as usize, POSITION_RESERVED);
        true
    }

    pub(crate) fn mark_planned(&mut self, piece: usize) -> Result<(), &'static str> {
        let position = *self
            .positions
            .get(piece)
            .ok_or("planned piece is outside picker geometry")?;
        match position {
            POSITION_PLANNED => return Err("piece is already planned"),
            POSITION_UNWANTED => return Err("planned piece is not wanted"),
            POSITION_RESERVED => self.positions[piece] = POSITION_PLANNED,
            POSITION_DETACHED => return Err("piece is detached from picker heap"),
            _ => {
                self.remove_at(position as usize, POSITION_PLANNED);
            }
        }
        Ok(())
    }

    pub(crate) fn mark_completed(&mut self, piece: usize) -> Result<(), &'static str> {
        let position = *self
            .positions
            .get(piece)
            .ok_or("completed piece is outside picker geometry")?;
        match position {
            POSITION_UNWANTED => return Err("completed piece is not wanted"),
            POSITION_RESERVED | POSITION_DETACHED => {
                return Err("completed piece is in a transient picker state");
            }
            POSITION_PLANNED => self.positions[piece] = POSITION_UNWANTED,
            _ => {
                self.remove_at(position as usize, POSITION_UNWANTED);
            }
        }
        self.wanted_remaining = self
            .wanted_remaining
            .checked_sub(1)
            .ok_or("wanted piece count underflow")?;
        Ok(())
    }

    pub(crate) const fn counters(&self) -> PiecePickerCounters {
        self.counters
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.counts
            .capacity()
            .saturating_mul(std::mem::size_of::<u16>())
            .saturating_add(
                self.heap
                    .capacity()
                    .saturating_mul(std::mem::size_of::<u32>()),
            )
            .saturating_add(
                self.positions
                    .capacity()
                    .saturating_mul(std::mem::size_of::<u32>()),
            )
    }

    #[cfg(test)]
    pub(crate) fn reset_counters(&mut self) {
        self.counters = PiecePickerCounters::default();
    }

    #[cfg(test)]
    fn naive_best_matching(&self, mut eligible: impl FnMut(usize) -> bool) -> Option<u32> {
        self.positions
            .iter()
            .enumerate()
            .filter(|(piece, position)| {
                **position < POSITION_DETACHED
                    && self.availability(*piece).is_some_and(|value| value != 0)
                    && eligible(*piece)
            })
            .map(|(piece, _)| piece as u32)
            .min_by(|lhs, rhs| self.rank(*lhs as usize, *rhs as usize))
    }

    fn repair_piece(&mut self, piece: usize) {
        let Some(&position) = self.positions.get(piece) else {
            return;
        };
        if position >= POSITION_DETACHED || position as usize >= self.ranked_len {
            return;
        }
        let position = self.sift_up(position as usize);
        self.sift_down(position);
    }

    fn rebuild_heap(&mut self, count_rebuild: bool) {
        if count_rebuild {
            self.counters.bulk_rebuilds = self.counters.bulk_rebuilds.saturating_add(1);
        }
        self.ranked_len = self.heap.len();
        self.eligibility_changed = false;
        for (position, piece) in self.heap.iter().copied().enumerate() {
            self.positions[piece as usize] = position as u32;
        }
        if self.ranked_len < 2 {
            return;
        }
        for position in (0..self.ranked_len / 2).rev() {
            self.sift_down(position);
        }
    }

    fn defer_ranked_root(&mut self) {
        debug_assert_ne!(self.ranked_len, 0);
        let last_ranked = self.ranked_len - 1;
        self.swap_heap(0, last_ranked);
        self.ranked_len -= 1;
        if self.ranked_len != 0 {
            self.sift_down(0);
        }
    }

    fn remove_ranked_root(&mut self, replacement_state: u32) -> u32 {
        debug_assert_ne!(self.ranked_len, 0);
        let root = self.heap[0];
        let last_ranked = self.ranked_len - 1;
        self.swap_heap(0, last_ranked);
        self.ranked_len -= 1;
        if self.ranked_len != 0 {
            self.sift_down(0);
        }
        let removed_position = self.ranked_len;
        let last = self.heap.len() - 1;
        self.swap_heap(removed_position, last);
        let removed = self.heap.pop().expect("ranked root exists");
        debug_assert_eq!(removed, root);
        self.positions[root as usize] = replacement_state;
        root
    }

    fn push_detached(&mut self, piece: u32) {
        debug_assert_eq!(self.positions[piece as usize], POSITION_DETACHED);
        self.heap.push(piece);
        let last = self.heap.len() - 1;
        self.positions[piece as usize] = last as u32;
        self.swap_heap(self.ranked_len, last);
        self.ranked_len += 1;
        self.sift_up(self.ranked_len - 1);
    }

    fn remove_at(&mut self, position: usize, replacement_state: u32) {
        let removed = self.heap[position];
        if position >= self.ranked_len {
            let last = self.heap.len() - 1;
            self.swap_heap(position, last);
            let popped = self.heap.pop().expect("deferred piece exists");
            debug_assert_eq!(popped, removed);
            self.positions[removed as usize] = replacement_state;
            return;
        }
        let last_ranked = self.ranked_len - 1;
        self.swap_heap(position, last_ranked);
        self.ranked_len -= 1;
        if position < self.ranked_len {
            let repaired = self.sift_up(position);
            self.sift_down(repaired);
        }
        let removed_position = self.ranked_len;
        let last = self.heap.len() - 1;
        self.swap_heap(removed_position, last);
        let popped = self.heap.pop().expect("ranked piece exists");
        debug_assert_eq!(popped, removed);
        self.positions[removed as usize] = replacement_state;
    }

    fn sift_up(&mut self, mut position: usize) -> usize {
        while position != 0 {
            let parent = (position - 1) / 2;
            if !self.heap_precedes(position, parent) {
                break;
            }
            self.swap_heap(position, parent);
            position = parent;
        }
        position
    }

    fn sift_down(&mut self, mut position: usize) {
        loop {
            let left = position.saturating_mul(2).saturating_add(1);
            if left >= self.ranked_len {
                break;
            }
            let right = left + 1;
            let best = if right < self.ranked_len && self.heap_precedes(right, left) {
                right
            } else {
                left
            };
            if !self.heap_precedes(best, position) {
                break;
            }
            self.swap_heap(position, best);
            position = best;
        }
    }

    fn heap_precedes(&mut self, lhs: usize, rhs: usize) -> bool {
        self.counters.rank_comparisons = self.counters.rank_comparisons.saturating_add(1);
        self.rank(self.heap[lhs] as usize, self.heap[rhs] as usize) == Ordering::Less
    }

    fn swap_heap(&mut self, lhs: usize, rhs: usize) {
        self.heap.swap(lhs, rhs);
        self.positions[self.heap[lhs] as usize] = lhs as u32;
        self.positions[self.heap[rhs] as usize] = rhs as u32;
    }

    fn rank(&self, lhs: usize, rhs: usize) -> Ordering {
        let lhs_availability = u32::from(self.counts[lhs]) + u32::from(self.seed_count);
        let rhs_availability = u32::from(self.counts[rhs]) + u32::from(self.seed_count);
        match (lhs_availability == 0, rhs_availability == 0) {
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            _ => {}
        }
        match self.policy {
            PieceActivationPolicy::InOrder => lhs.cmp(&rhs),
            PieceActivationPolicy::RarestFirst => lhs_availability
                .cmp(&rhs_availability)
                .then_with(|| self.tie_key(lhs).cmp(&self.tie_key(rhs)))
                .then_with(|| lhs.cmp(&rhs)),
        }
    }

    fn tie_key(&self, piece: usize) -> u64 {
        if self.tie_seed == 0 {
            return piece as u64;
        }
        let offset = (self.tie_seed % self.counts.len() as u64) as usize;
        ((piece + self.counts.len() - offset) % self.counts.len()) as u64
    }
}

pub(crate) fn picker_seed(info_hash: [u8; 20], peer_id: [u8; 20]) -> u64 {
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    for byte in info_hash.into_iter().chain(peer_id) {
        state ^= u64::from(byte);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    state
}

#[cfg(test)]
fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    const MAX_PIECES: usize = 2_097_152;

    #[test]
    fn in_order_and_rarest_first_share_eligibility_but_not_rank() {
        let wanted = (0..8).collect::<Vec<_>>();
        let mut in_order =
            AvailabilityPicker::new(8, wanted.clone(), PieceActivationPolicy::InOrder, 0)
                .expect("in-order picker");
        let mut rarest = AvailabilityPicker::new(8, wanted, PieceActivationPolicy::RarestFirst, 0)
            .expect("rarest picker");
        for piece in 0..8 {
            in_order.increment_piece(piece).expect("availability");
            rarest.increment_piece(piece).expect("availability");
        }
        rarest.increment_piece(0).expect("common piece");
        assert_eq!(in_order.reserve_best_matching(|_| true), Some(0));
        assert_eq!(rarest.reserve_best_matching(|_| true), Some(1));
    }

    #[test]
    fn equal_rarity_uses_a_seeded_contiguous_rotation() {
        let mut picker =
            AvailabilityPicker::new(8, (0..8).collect(), PieceActivationPolicy::RarestFirst, 3)
                .expect("rotated picker");
        for piece in 0..8 {
            picker.increment_piece(piece).expect("availability");
        }
        for expected in [3, 4, 5, 6, 7, 0, 1, 2] {
            let selected = picker.reserve_best_matching(|_| true).expect("selection");
            assert_eq!(selected, expected);
            picker
                .mark_planned(selected as usize)
                .expect("consume selection");
        }
    }

    #[test]
    fn optimized_picker_matches_naive_oracle_across_seeded_transition_trace() {
        const PIECES: usize = 1_024;
        let mut picker = AvailabilityPicker::new(
            PIECES,
            (0..PIECES as u32).collect(),
            PieceActivationPolicy::RarestFirst,
            0x91_5eed,
        )
        .expect("picker");
        let mut state = 0x4d59_5df4_d0f3_3173_u64;
        for step in 0..10_000 {
            state = splitmix64(state ^ step);
            let piece = state as usize % PIECES;
            if state & 1 == 0 {
                if picker.counts[piece] < 30 {
                    picker.increment_piece(piece).expect("increment");
                }
            } else if picker.counts[piece] != 0 {
                picker.decrement_piece(piece).expect("decrement");
            }
            let expected = picker.naive_best_matching(|candidate| candidate % 7 != 6);
            let actual = picker.reserve_best_matching(|candidate| candidate % 7 != 6);
            assert_eq!(actual, expected, "transition {step}");
            if let Some(piece) = actual {
                picker.cancel_reserved(piece as usize).expect("restore");
            }
        }
    }

    #[test]
    fn maximum_geometry_retains_bounded_memory_and_promotes_a_tail_piece() {
        let started = Instant::now();
        let mut picker = AvailabilityPicker::new(
            MAX_PIECES,
            (0..MAX_PIECES as u32).collect(),
            PieceActivationPolicy::RarestFirst,
            0x91_5eed,
        )
        .expect("maximum picker");
        let retained = picker.retained_bytes();
        assert!(retained <= MAX_PIECES * 12, "retained {retained} bytes");
        picker.reset_counters();
        picker
            .increment_piece(MAX_PIECES - 1)
            .expect("tail availability");
        assert_eq!(
            picker.reserve_best_matching(|_| true),
            Some((MAX_PIECES - 1) as u32)
        );
        let counters = picker.counters();
        assert_eq!(counters.single_piece_updates, 1);
        assert_eq!(counters.bulk_rebuilds, 0);
        assert_eq!(counters.candidate_inspections, 1);
        assert!(
            counters.rank_comparisons <= 4 * (usize::BITS - MAX_PIECES.leading_zeros()) as u64 + 4,
            "{} comparisons",
            counters.rank_comparisons,
        );
        assert!(started.elapsed().as_secs() < 30);
    }

    #[test]
    fn connection_filtering_inspects_a_bounded_candidate_prefix() {
        let piece_count = MAX_PICKER_CANDIDATE_INSPECTIONS * 4;
        let mut picker = AvailabilityPicker::new(
            piece_count,
            (0..piece_count as u32).collect(),
            PieceActivationPolicy::RarestFirst,
            0,
        )
        .expect("picker");
        for piece in 0..piece_count {
            picker.increment_piece(piece).expect("availability");
        }
        picker.reset_counters();
        assert_eq!(
            picker.reserve_best_matching(|piece| piece >= MAX_PICKER_CANDIDATE_INSPECTIONS),
            None
        );
        assert_eq!(
            picker.counters().candidate_inspections,
            MAX_PICKER_CANDIDATE_INSPECTIONS as u64
        );
        assert_eq!(picker.heap.len(), piece_count);
        assert_eq!(
            picker.reserve_best_matching(|piece| piece >= MAX_PICKER_CANDIDATE_INSPECTIONS),
            Some(MAX_PICKER_CANDIDATE_INSPECTIONS as u32)
        );
        assert_eq!(
            picker.counters().candidate_inspections,
            MAX_PICKER_CANDIDATE_INSPECTIONS as u64 + 1
        );
    }

    #[test]
    #[ignore = "manual release-mode picker timing profile"]
    fn maximum_geometry_timing_profile() {
        for piece_count in [131_072, 524_288, MAX_PIECES] {
            let started = Instant::now();
            let mut picker = AvailabilityPicker::new(
                piece_count,
                (0..piece_count as u32).collect(),
                PieceActivationPolicy::RarestFirst,
                0x91_5eed,
            )
            .expect("profile picker");
            let build = started.elapsed();
            let update_started = Instant::now();
            for piece in (0..piece_count).step_by(2) {
                picker
                    .increment_piece_without_repair(piece)
                    .expect("bulk increment");
            }
            picker.rebuild_after_bulk_update();
            let rebuild = update_started.elapsed();
            println!(
                "pieces={piece_count} retained_bytes={} build_ms={} rebuild_ms={} build_ns_per_piece={} rebuild_ns_per_piece={} comparisons={}",
                picker.retained_bytes(),
                build.as_millis(),
                rebuild.as_millis(),
                build.as_nanos() / piece_count as u128,
                rebuild.as_nanos() / piece_count as u128,
                picker.counters().rank_comparisons,
            );
            assert!(picker.retained_bytes() <= piece_count * 12);
        }
    }

    #[test]
    #[ignore = "manual release-mode four-torrent memory profile"]
    fn maximum_geometry_four_torrent_profile() {
        let started = Instant::now();
        let mut pickers = (0..4)
            .map(|torrent| {
                AvailabilityPicker::new(
                    MAX_PIECES,
                    (0..MAX_PIECES as u32).collect(),
                    PieceActivationPolicy::RarestFirst,
                    0x91_5eed + torrent,
                )
                .expect("maximum picker")
            })
            .collect::<Vec<_>>();
        let retained = pickers
            .iter()
            .map(AvailabilityPicker::retained_bytes)
            .sum::<usize>();
        for (torrent, picker) in pickers.iter_mut().enumerate() {
            picker
                .increment_piece(MAX_PIECES - 1 - torrent)
                .expect("tail availability");
            assert!(picker.reserve_best_matching(|_| true).is_some());
        }
        println!(
            "torrents=4 pieces_each={MAX_PIECES} retained_bytes={retained} build_and_query_ms={}",
            started.elapsed().as_millis(),
        );
        assert!(retained <= 4 * MAX_PIECES * 12);
    }
}

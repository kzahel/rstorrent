//! Runtime-independent complete-seed upload-slot scheduling.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub const DEFAULT_UNCHOKE_SLOTS: usize = 8;
pub const DEFAULT_UNCHOKE_INTERVAL: Duration = Duration::from_secs(15);
pub const DEFAULT_OPTIMISTIC_UNCHOKE_INTERVAL: Duration = Duration::from_secs(30);
pub const DEFAULT_SEEDING_PIECE_QUOTA: u64 = 20;

const MIN_SEED_GRANT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UploadPeerId(u64);

impl UploadPeerId {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadSchedulerConfig {
    pub slots: usize,
    pub unchoke_interval: Duration,
    pub optimistic_interval: Duration,
    pub seeding_piece_quota: u64,
}

impl Default for UploadSchedulerConfig {
    fn default() -> Self {
        Self {
            slots: DEFAULT_UNCHOKE_SLOTS,
            unchoke_interval: DEFAULT_UNCHOKE_INTERVAL,
            optimistic_interval: DEFAULT_OPTIMISTIC_UNCHOKE_INTERVAL,
            seeding_piece_quota: DEFAULT_SEEDING_PIECE_QUOTA,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadSchedulerPeer {
    pub id: UploadPeerId,
    pub torrent: [u8; 20],
    pub piece_length: u32,
    pub interested: bool,
    pub payload_uploaded: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadGrant {
    Choked,
    Regular,
    Optimistic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadDecision {
    pub peer: UploadPeerId,
    pub grant: UploadGrant,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UploadSchedulerSnapshot {
    pub peers: usize,
    pub interested: usize,
    pub regular: usize,
    pub optimistic: usize,
    pub evaluations: u64,
    pub optimistic_rotations: u64,
}

#[derive(Clone, Copy, Debug)]
struct PeerState {
    input: UploadSchedulerPeer,
    grant: UploadGrant,
    uploaded_at_last_round: u64,
    uploaded_at_unchoke: u64,
    last_unchoke: Duration,
    last_optimistic: Option<Duration>,
}

impl PeerState {
    fn new(input: UploadSchedulerPeer, now: Duration) -> Self {
        Self {
            input,
            grant: UploadGrant::Choked,
            uploaded_at_last_round: input.payload_uploaded,
            uploaded_at_unchoke: input.payload_uploaded,
            last_unchoke: now,
            last_optimistic: None,
        }
    }

    fn uploaded_in_last_round(self) -> u64 {
        self.input
            .payload_uploaded
            .saturating_sub(self.uploaded_at_last_round)
    }

    fn quota_complete(self, now: Duration, pieces: u64) -> bool {
        self.grant != UploadGrant::Choked
            && self
                .input
                .payload_uploaded
                .saturating_sub(self.uploaded_at_unchoke)
                > u64::from(self.input.piece_length).saturating_mul(pieces)
            && now.saturating_sub(self.last_unchoke) > MIN_SEED_GRANT
    }
}

#[derive(Debug)]
pub struct UploadScheduler {
    config: UploadSchedulerConfig,
    peers: BTreeMap<UploadPeerId, PeerState>,
    last_ordinary: Option<Duration>,
    last_optimistic: Option<Duration>,
    evaluations: u64,
    optimistic_rotations: u64,
}

impl UploadScheduler {
    pub fn new(config: UploadSchedulerConfig) -> Result<Self, &'static str> {
        if config.unchoke_interval.is_zero() || config.optimistic_interval.is_zero() {
            return Err("upload scheduler intervals must be nonzero");
        }
        if config.seeding_piece_quota == 0 {
            return Err("seeding piece quota must be nonzero");
        }
        Ok(Self {
            config,
            peers: BTreeMap::new(),
            last_ordinary: None,
            last_optimistic: None,
            evaluations: 0,
            optimistic_rotations: 0,
        })
    }

    pub fn libtorrent_default() -> Self {
        Self::new(UploadSchedulerConfig::default()).expect("default upload scheduler is valid")
    }

    pub fn update_peer(&mut self, peer: UploadSchedulerPeer, now: Duration) {
        self.peers
            .entry(peer.id)
            .and_modify(|state| state.input = peer)
            .or_insert_with(|| PeerState::new(peer, now));
    }

    pub fn remove_peer(&mut self, peer: UploadPeerId) {
        self.peers.remove(&peer);
    }

    pub fn evaluate(&mut self, now: Duration) -> Vec<UploadDecision> {
        self.evaluations = self.evaluations.saturating_add(1);
        let old = self
            .peers
            .iter()
            .map(|(id, state)| (*id, state.grant))
            .collect::<BTreeMap<_, _>>();

        if self.config.slots == 0 {
            for state in self.peers.values_mut() {
                state.grant = UploadGrant::Choked;
            }
            return decisions(&old, &self.peers);
        }

        let ordinary_due = self
            .last_ordinary
            .is_none_or(|last| now.saturating_sub(last) >= self.config.unchoke_interval);
        let optimistic_due = self
            .last_optimistic
            .is_none_or(|last| now.saturating_sub(last) >= self.config.optimistic_interval);
        let optimistic_slots = automatic_optimistic_slots(self.config.slots);
        let regular_slots = self.config.slots.saturating_sub(optimistic_slots);

        let mut regular = self
            .peers
            .iter()
            .filter_map(|(id, state)| {
                (state.input.interested
                    && state.grant == UploadGrant::Regular
                    && (!ordinary_due
                        || !state.quota_complete(now, self.config.seeding_piece_quota)))
                .then_some(*id)
            })
            .collect::<Vec<_>>();
        regular.sort_by(|left, right| self.compare_regular(*left, *right, now));
        regular.truncate(regular_slots);

        if ordinary_due || regular.len() < regular_slots {
            let selected = regular.iter().copied().collect::<BTreeSet<_>>();
            let mut candidates = self
                .peers
                .keys()
                .copied()
                .filter(|id| self.peers[id].input.interested && !selected.contains(id))
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| self.compare_regular(*left, *right, now));
            regular.extend(candidates.into_iter().take(regular_slots - regular.len()));
        }
        let regular = regular.into_iter().collect::<BTreeSet<_>>();

        let current_optimistic = self.peers.iter().find_map(|(id, state)| {
            (state.grant == UploadGrant::Optimistic
                && state.input.interested
                && !regular.contains(id))
            .then_some(*id)
        });
        let optimistic = if optimistic_slots == 0 {
            None
        } else if !optimistic_due && current_optimistic.is_some() {
            current_optimistic
        } else {
            let mut candidates = self
                .peers
                .iter()
                .filter_map(|(id, state)| {
                    (state.input.interested && !regular.contains(id))
                        .then_some((*id, state.last_optimistic))
                })
                .collect::<Vec<_>>();
            candidates.sort_by_key(|(id, last)| (*last, *id));
            candidates.first().map(|(id, _)| *id)
        };

        if ordinary_due {
            self.last_ordinary = Some(now);
        }
        if optimistic_due {
            self.last_optimistic = Some(now);
            if optimistic.is_some() {
                self.optimistic_rotations = self.optimistic_rotations.saturating_add(1);
            }
        }

        for (id, state) in &mut self.peers {
            let next = if regular.contains(id) {
                UploadGrant::Regular
            } else if Some(*id) == optimistic {
                UploadGrant::Optimistic
            } else {
                UploadGrant::Choked
            };
            if next != UploadGrant::Choked && state.grant == UploadGrant::Choked {
                state.last_unchoke = now;
                state.uploaded_at_unchoke = state.input.payload_uploaded;
            }
            if next == UploadGrant::Optimistic && state.grant != UploadGrant::Optimistic {
                state.last_optimistic = Some(now);
            }
            state.grant = next;
            if ordinary_due {
                state.uploaded_at_last_round = state.input.payload_uploaded;
            }
        }

        decisions(&old, &self.peers)
    }

    pub fn grant(&self, peer: UploadPeerId) -> Option<UploadGrant> {
        self.peers.get(&peer).map(|state| state.grant)
    }

    pub fn snapshot(&self) -> UploadSchedulerSnapshot {
        UploadSchedulerSnapshot {
            peers: self.peers.len(),
            interested: self
                .peers
                .values()
                .filter(|state| state.input.interested)
                .count(),
            regular: self
                .peers
                .values()
                .filter(|state| state.grant == UploadGrant::Regular)
                .count(),
            optimistic: self
                .peers
                .values()
                .filter(|state| state.grant == UploadGrant::Optimistic)
                .count(),
            evaluations: self.evaluations,
            optimistic_rotations: self.optimistic_rotations,
        }
    }

    fn compare_regular(&self, left: UploadPeerId, right: UploadPeerId, now: Duration) -> Ordering {
        let left = self.peers[&left];
        let right = self.peers[&right];
        left.quota_complete(now, self.config.seeding_piece_quota)
            .cmp(&right.quota_complete(now, self.config.seeding_piece_quota))
            .then_with(|| {
                let left_rate = if left.grant == UploadGrant::Choked {
                    0
                } else {
                    left.uploaded_in_last_round()
                };
                let right_rate = if right.grant == UploadGrant::Choked {
                    0
                } else {
                    right.uploaded_in_last_round()
                };
                right_rate.cmp(&left_rate)
            })
            .then_with(|| left.last_unchoke.cmp(&right.last_unchoke))
            .then_with(|| left.input.id.cmp(&right.input.id))
    }
}

impl Default for UploadScheduler {
    fn default() -> Self {
        Self::libtorrent_default()
    }
}

const fn automatic_optimistic_slots(slots: usize) -> usize {
    if slots == 0 {
        0
    } else if slots / 5 == 0 {
        1
    } else {
        slots / 5
    }
}

fn decisions(
    old: &BTreeMap<UploadPeerId, UploadGrant>,
    peers: &BTreeMap<UploadPeerId, PeerState>,
) -> Vec<UploadDecision> {
    peers
        .iter()
        .filter_map(|(id, state)| {
            (old.get(id).copied().unwrap_or(UploadGrant::Choked) != state.grant).then_some(
                UploadDecision {
                    peer: *id,
                    grant: state.grant,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        DEFAULT_OPTIMISTIC_UNCHOKE_INTERVAL, DEFAULT_SEEDING_PIECE_QUOTA, DEFAULT_UNCHOKE_INTERVAL,
        DEFAULT_UNCHOKE_SLOTS, UploadGrant, UploadPeerId, UploadScheduler, UploadSchedulerConfig,
        UploadSchedulerPeer, automatic_optimistic_slots,
    };

    fn id(value: u64) -> UploadPeerId {
        UploadPeerId::new(value).expect("nonzero peer id")
    }

    fn peer(value: u64, uploaded: u64) -> UploadSchedulerPeer {
        UploadSchedulerPeer {
            id: id(value),
            torrent: [value as u8; 20],
            piece_length: 16_384,
            interested: true,
            payload_uploaded: uploaded,
        }
    }

    #[test]
    fn defaults_match_pinned_libtorrent() {
        let config = UploadSchedulerConfig::default();
        assert_eq!(config.slots, DEFAULT_UNCHOKE_SLOTS);
        assert_eq!(config.unchoke_interval, DEFAULT_UNCHOKE_INTERVAL);
        assert_eq!(
            config.optimistic_interval,
            DEFAULT_OPTIMISTIC_UNCHOKE_INTERVAL
        );
        assert_eq!(config.seeding_piece_quota, DEFAULT_SEEDING_PIECE_QUOTA);
        assert_eq!(automatic_optimistic_slots(8), 1);
        assert_eq!(automatic_optimistic_slots(10), 2);
        assert_eq!(automatic_optimistic_slots(0), 0);
    }

    #[test]
    fn eight_slots_include_one_optimistic_and_only_interested_peers() {
        let mut scheduler = UploadScheduler::default();
        for value in 1..=10 {
            let mut input = peer(value, 0);
            input.interested = value != 10;
            scheduler.update_peer(input, Duration::ZERO);
        }
        scheduler.evaluate(Duration::ZERO);
        let snapshot = scheduler.snapshot();
        assert_eq!(snapshot.interested, 9);
        assert_eq!(snapshot.regular, 7);
        assert_eq!(snapshot.optimistic, 1);
        assert_eq!(scheduler.grant(id(10)), Some(UploadGrant::Choked));
    }

    #[test]
    fn vacancies_fill_before_the_periodic_interval() {
        let mut scheduler = UploadScheduler::new(UploadSchedulerConfig {
            slots: 2,
            ..UploadSchedulerConfig::default()
        })
        .expect("valid config");
        scheduler.update_peer(peer(1, 0), Duration::ZERO);
        scheduler.evaluate(Duration::ZERO);
        scheduler.update_peer(peer(2, 0), Duration::from_secs(1));
        scheduler.evaluate(Duration::from_secs(1));
        assert_eq!(scheduler.snapshot().regular, 1);
        assert_eq!(scheduler.snapshot().optimistic, 1);
    }

    #[test]
    fn optimistic_peer_rotates_at_thirty_seconds() {
        let mut scheduler = UploadScheduler::new(UploadSchedulerConfig {
            slots: 1,
            ..UploadSchedulerConfig::default()
        })
        .expect("valid config");
        for value in 1..=3 {
            scheduler.update_peer(peer(value, 0), Duration::ZERO);
        }
        scheduler.evaluate(Duration::ZERO);
        assert_eq!(scheduler.grant(id(1)), Some(UploadGrant::Optimistic));
        scheduler.evaluate(Duration::from_secs(29));
        assert_eq!(scheduler.grant(id(1)), Some(UploadGrant::Optimistic));
        scheduler.evaluate(Duration::from_secs(30));
        assert_eq!(scheduler.grant(id(2)), Some(UploadGrant::Optimistic));
        scheduler.evaluate(Duration::from_secs(60));
        assert_eq!(scheduler.grant(id(3)), Some(UploadGrant::Optimistic));
    }

    #[test]
    fn seed_quota_requires_strict_bytes_and_time_boundaries() {
        let mut scheduler = UploadScheduler::new(UploadSchedulerConfig {
            slots: 2,
            ..UploadSchedulerConfig::default()
        })
        .expect("valid config");
        scheduler.update_peer(peer(1, 0), Duration::ZERO);
        scheduler.update_peer(peer(2, 0), Duration::ZERO);
        scheduler.update_peer(peer(3, 0), Duration::ZERO);
        scheduler.evaluate(Duration::ZERO);
        assert_eq!(scheduler.grant(id(1)), Some(UploadGrant::Regular));

        let exact_quota = 16_384 * DEFAULT_SEEDING_PIECE_QUOTA;
        scheduler.update_peer(peer(1, exact_quota), Duration::from_secs(60));
        scheduler.evaluate(Duration::from_secs(60));
        assert_eq!(scheduler.grant(id(1)), Some(UploadGrant::Regular));

        scheduler.update_peer(peer(1, exact_quota + 1), Duration::from_secs(61));
        scheduler.evaluate(Duration::from_secs(75));
        assert_eq!(scheduler.grant(id(1)), Some(UploadGrant::Choked));
        assert_eq!(scheduler.snapshot().regular, 1);
    }

    #[test]
    fn zero_slots_chokes_every_peer() {
        let mut scheduler = UploadScheduler::new(UploadSchedulerConfig {
            slots: 1,
            ..UploadSchedulerConfig::default()
        })
        .expect("valid config");
        scheduler.update_peer(peer(1, 0), Duration::ZERO);
        scheduler.evaluate(Duration::ZERO);
        assert_eq!(scheduler.grant(id(1)), Some(UploadGrant::Optimistic));

        scheduler.config.slots = 0;
        let decisions = scheduler.evaluate(Duration::from_secs(1));
        assert_eq!(decisions.len(), 1);
        assert_eq!(scheduler.grant(id(1)), Some(UploadGrant::Choked));
    }
}

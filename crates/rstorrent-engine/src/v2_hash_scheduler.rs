//! Task-free torrent ownership for bounded BEP 52 hash attempts.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rstorrent_protocol::v2_hashes::{
    HashExchangeError, HashRequest, HashResponse, V2FileHashGeometry, V2HashCatalog,
};

use crate::swarm::ConnectionId;

pub(crate) const MAX_HASH_ATTEMPTS_PER_PEER: usize = 2;
pub(crate) const MAX_HASH_ATTEMPTS_PER_TORRENT: usize = 16;
pub(crate) const MAX_HASH_ATTEMPTS_PER_RANGE: usize = 2;
pub(crate) const HASH_DUPLICATE_DELAY: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HashNeedInput {
    pub geometry: V2FileHashGeometry,
    pub request: HashRequest,
    pub piece: u32,
    pub candidate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HashAttempt {
    issued_at: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LogicalHashNeed {
    geometry: V2FileHashGeometry,
    fresh_pieces: BTreeSet<u32>,
    candidate_pieces: BTreeSet<u32>,
    attempts: BTreeMap<ConnectionId, HashAttempt>,
    complete: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HashAssignment {
    pub connection: ConnectionId,
    pub request: HashRequest,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AuthenticatedPieces {
    pub fresh: Vec<u32>,
    pub candidates: Vec<u32>,
    pub inserted_hashes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HashResponseDisposition {
    Accepted(AuthenticatedPieces),
    BadProof(HashExchangeError),
    Unsolicited,
    Mismatched,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HashRejectDisposition {
    Accepted,
    Unsolicited,
    Mismatched,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct V2HashSchedulerSnapshot {
    pub logical_needs: usize,
    pub active_attempts: usize,
    pub active_attempts_high_water: usize,
    pub duplicate_attempts: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct V2HashScheduler {
    needs: BTreeMap<HashRequest, LogicalHashNeed>,
    active_attempts_high_water: usize,
}

impl V2HashScheduler {
    pub(crate) fn new(
        inputs: impl IntoIterator<Item = HashNeedInput>,
    ) -> Result<Self, &'static str> {
        let mut scheduler = Self::default();
        for input in inputs {
            let need = scheduler
                .needs
                .entry(input.request)
                .or_insert_with(|| LogicalHashNeed {
                    geometry: input.geometry,
                    fresh_pieces: BTreeSet::new(),
                    candidate_pieces: BTreeSet::new(),
                    attempts: BTreeMap::new(),
                    complete: false,
                });
            if need.geometry != input.geometry {
                return Err("one v2 hash request resolved to conflicting file geometry");
            }
            if input.candidate {
                need.candidate_pieces.insert(input.piece);
            } else {
                need.fresh_pieces.insert(input.piece);
            }
        }
        Ok(scheduler)
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.needs.values().all(|need| need.complete)
    }

    #[cfg(test)]
    pub(crate) fn schedule(
        &mut self,
        now: Duration,
        eligible: impl FnMut(&[u32]) -> Vec<ConnectionId>,
    ) -> Vec<HashAssignment> {
        self.schedule_with_capacity(now, MAX_HASH_ATTEMPTS_PER_TORRENT, eligible)
    }

    #[cfg(test)]
    pub(crate) fn schedule_with_capacity(
        &mut self,
        now: Duration,
        maximum_total_attempts: usize,
        eligible: impl FnMut(&[u32]) -> Vec<ConnectionId>,
    ) -> Vec<HashAssignment> {
        self.schedule_with_reservations(now, maximum_total_attempts, eligible, |_| 0)
    }

    pub(crate) fn schedule_with_reservations(
        &mut self,
        now: Duration,
        maximum_total_attempts: usize,
        mut eligible: impl FnMut(&[u32]) -> Vec<ConnectionId>,
        mut reserved_peer_attempts: impl FnMut(ConnectionId) -> usize,
    ) -> Vec<HashAssignment> {
        let mut assignments = Vec::new();
        let mut peer_counts = self.peer_attempt_counts();
        let mut total = peer_counts.values().sum::<usize>();
        let maximum_total_attempts = maximum_total_attempts.min(MAX_HASH_ATTEMPTS_PER_TORRENT);
        let mut keys = self
            .needs
            .iter()
            .filter(|(_, need)| !need.complete)
            .map(|(request, need)| {
                (
                    need.fresh_pieces.is_empty(),
                    need.fresh_pieces
                        .iter()
                        .chain(&need.candidate_pieces)
                        .next()
                        .copied()
                        .unwrap_or(u32::MAX),
                    *request,
                )
            })
            .collect::<Vec<_>>();
        keys.sort_unstable();
        for (_, _, request) in keys {
            if total >= maximum_total_attempts {
                break;
            }
            let need = self
                .needs
                .get_mut(&request)
                .expect("collected hash need remains present");
            if need.attempts.len() >= MAX_HASH_ATTEMPTS_PER_RANGE {
                continue;
            }
            if let Some(oldest) = need
                .attempts
                .values()
                .map(|attempt| attempt.issued_at)
                .min()
                && now.saturating_sub(oldest) < HASH_DUPLICATE_DELAY
            {
                continue;
            }
            let pieces = need
                .fresh_pieces
                .iter()
                .chain(&need.candidate_pieces)
                .copied()
                .collect::<Vec<_>>();
            let Some(connection) = eligible(&pieces).into_iter().find(|connection| {
                !need.attempts.contains_key(connection)
                    && peer_counts
                        .get(connection)
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(reserved_peer_attempts(*connection))
                        < MAX_HASH_ATTEMPTS_PER_PEER
            }) else {
                continue;
            };
            need.attempts
                .insert(connection, HashAttempt { issued_at: now });
            *peer_counts.entry(connection).or_default() += 1;
            total += 1;
            self.active_attempts_high_water = self.active_attempts_high_water.max(total);
            assignments.push(HashAssignment {
                connection,
                request,
            });
        }
        assignments
    }

    pub(crate) fn send_failed(&mut self, connection: ConnectionId, request: HashRequest) {
        if let Some(need) = self.needs.get_mut(&request) {
            need.attempts.remove(&connection);
        }
        self.prune_completed();
    }

    pub(crate) fn receive_response(
        &mut self,
        connection: ConnectionId,
        response: &HashResponse,
        catalog: &mut V2HashCatalog,
    ) -> HashResponseDisposition {
        let Some(need) = self.needs.get_mut(&response.request) else {
            return if self.peer_has_attempt(connection) {
                HashResponseDisposition::Mismatched
            } else {
                HashResponseDisposition::Unsolicited
            };
        };
        if !need.attempts.contains_key(&connection) {
            return if self.peer_has_attempt(connection) {
                HashResponseDisposition::Mismatched
            } else {
                HashResponseDisposition::Unsolicited
            };
        }
        let inserted_hashes = match catalog.insert_response(need.geometry, response) {
            Ok(inserted) => inserted,
            Err(error) => {
                need.attempts.remove(&connection);
                return HashResponseDisposition::BadProof(error);
            }
        };
        need.attempts.remove(&connection);
        let authenticated = if need.complete {
            AuthenticatedPieces {
                inserted_hashes,
                ..AuthenticatedPieces::default()
            }
        } else {
            need.complete = true;
            AuthenticatedPieces {
                fresh: need.fresh_pieces.iter().copied().collect(),
                candidates: need.candidate_pieces.iter().copied().collect(),
                inserted_hashes,
            }
        };
        self.prune_completed();
        HashResponseDisposition::Accepted(authenticated)
    }

    pub(crate) fn receive_reject(
        &mut self,
        connection: ConnectionId,
        request: HashRequest,
    ) -> HashRejectDisposition {
        let Some(need) = self.needs.get_mut(&request) else {
            return if self.peer_has_attempt(connection) {
                HashRejectDisposition::Mismatched
            } else {
                HashRejectDisposition::Unsolicited
            };
        };
        if need.attempts.remove(&connection).is_none() {
            return if self.peer_has_attempt(connection) {
                HashRejectDisposition::Mismatched
            } else {
                HashRejectDisposition::Unsolicited
            };
        }
        self.prune_completed();
        HashRejectDisposition::Accepted
    }

    pub(crate) fn peer_disconnected(&mut self, connection: ConnectionId) {
        for need in self.needs.values_mut() {
            need.attempts.remove(&connection);
        }
        self.prune_completed();
    }

    pub(crate) fn retain_connections(&mut self, mut retained: impl FnMut(ConnectionId) -> bool) {
        for need in self.needs.values_mut() {
            need.attempts.retain(|connection, _| retained(*connection));
        }
        self.prune_completed();
    }

    pub(crate) fn snapshot(&self) -> V2HashSchedulerSnapshot {
        let active_attempts = self.needs.values().map(|need| need.attempts.len()).sum();
        V2HashSchedulerSnapshot {
            logical_needs: self.needs.values().filter(|need| !need.complete).count(),
            active_attempts,
            active_attempts_high_water: self.active_attempts_high_water,
            duplicate_attempts: self
                .needs
                .values()
                .map(|need| need.attempts.len().saturating_sub(1))
                .sum(),
        }
    }

    pub(crate) fn active_attempts(&self) -> usize {
        self.needs.values().map(|need| need.attempts.len()).sum()
    }

    pub(crate) fn peer_attempt_count(&self, connection: ConnectionId) -> usize {
        self.needs
            .values()
            .filter(|need| need.attempts.contains_key(&connection))
            .count()
    }

    pub(crate) fn owns_attempt(&self, connection: ConnectionId, request: HashRequest) -> bool {
        self.needs
            .get(&request)
            .is_some_and(|need| need.attempts.contains_key(&connection))
    }

    fn peer_attempt_counts(&self) -> BTreeMap<ConnectionId, usize> {
        let mut counts = BTreeMap::new();
        for need in self.needs.values() {
            for connection in need.attempts.keys() {
                *counts.entry(*connection).or_default() += 1;
            }
        }
        counts
    }

    fn peer_has_attempt(&self, connection: ConnectionId) -> bool {
        self.needs
            .values()
            .any(|need| need.attempts.contains_key(&connection))
    }

    fn prune_completed(&mut self) {
        self.needs
            .retain(|_, need| !need.complete || !need.attempts.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use rstorrent_protocol::merkle::{file_root_from_piece_hashes, hash_block, hash_pair};

    use super::*;

    fn connection(value: u64) -> ConnectionId {
        ConnectionId::new(value).unwrap()
    }

    fn fixture() -> (V2FileHashGeometry, HashRequest, HashResponse) {
        let roots = [b"a", b"b", b"c"].map(|value| hash_block(value).unwrap());
        let pieces_root = file_root_from_piece_hashes(roots, 16 * 1024).unwrap();
        let geometry =
            V2FileHashGeometry::new(pieces_root, 3 * 16 * 1024, 16 * 1024, 0, 3).unwrap();
        let request = HashRequest {
            pieces_root,
            base_layer: 0,
            index: 0,
            count: 2,
            proof_layers: 2,
        };
        let response = HashResponse {
            request,
            hashes: vec![roots[0], roots[1], hash_pair(&roots[2], &[0; 32])],
        };
        (geometry, request, response)
    }

    #[test]
    fn stalls_duplicate_once_and_peer_limits_remain_exact() {
        let (geometry, request, _) = fixture();
        let mut scheduler = V2HashScheduler::new([
            HashNeedInput {
                geometry,
                request,
                piece: 0,
                candidate: false,
            },
            HashNeedInput {
                geometry,
                request,
                piece: 1,
                candidate: false,
            },
        ])
        .unwrap();
        assert_eq!(
            scheduler.schedule(Duration::ZERO, |_| vec![connection(1), connection(2)]),
            vec![HashAssignment {
                connection: connection(1),
                request
            }]
        );
        assert!(
            scheduler
                .schedule(Duration::from_secs(2), |_| vec![connection(2)])
                .is_empty()
        );
        assert_eq!(
            scheduler.schedule(Duration::from_secs(3), |_| vec![connection(2)]),
            vec![HashAssignment {
                connection: connection(2),
                request
            }]
        );
        assert!(
            scheduler
                .schedule(Duration::from_secs(30), |_| vec![connection(3)])
                .is_empty()
        );
        assert_eq!(scheduler.snapshot().active_attempts, 2);
        assert_eq!(scheduler.snapshot().duplicate_attempts, 1);
    }

    #[test]
    fn valid_first_and_late_duplicate_are_correlated_and_idempotent() {
        let (geometry, request, response) = fixture();
        let mut scheduler = V2HashScheduler::new([HashNeedInput {
            geometry,
            request,
            piece: 0,
            candidate: true,
        }])
        .unwrap();
        scheduler.schedule(Duration::ZERO, |_| vec![connection(1)]);
        scheduler.schedule(Duration::from_secs(3), |_| vec![connection(2)]);
        let mut catalog = V2HashCatalog::new(3).unwrap();
        assert_eq!(
            scheduler.receive_response(connection(1), &response, &mut catalog),
            HashResponseDisposition::Accepted(AuthenticatedPieces {
                candidates: vec![0],
                inserted_hashes: 2,
                ..AuthenticatedPieces::default()
            })
        );
        assert!(scheduler.is_complete());
        assert_eq!(
            scheduler.receive_response(connection(2), &response, &mut catalog),
            HashResponseDisposition::Accepted(AuthenticatedPieces::default())
        );
        assert_eq!(scheduler.snapshot().active_attempts, 0);
    }

    #[test]
    fn rejection_disconnect_bad_proof_and_mismatch_never_adopt_truth() {
        let (geometry, request, mut response) = fixture();
        let mut scheduler = V2HashScheduler::new([HashNeedInput {
            geometry,
            request,
            piece: 0,
            candidate: false,
        }])
        .unwrap();
        scheduler.schedule(Duration::ZERO, |_| vec![connection(1)]);
        assert_eq!(
            scheduler.receive_reject(connection(1), request),
            HashRejectDisposition::Accepted
        );
        scheduler.schedule(Duration::ZERO, |_| vec![connection(2)]);
        response.hashes[0] = [9; 32];
        let mut catalog = V2HashCatalog::new(3).unwrap();
        assert!(matches!(
            scheduler.receive_response(connection(2), &response, &mut catalog),
            HashResponseDisposition::BadProof(HashExchangeError::BadProof)
        ));
        assert_eq!(catalog.piece_root(0), None);
        scheduler.schedule(Duration::ZERO, |_| vec![connection(3)]);
        let mut other = response;
        other.request.index = 2;
        assert_eq!(
            scheduler.receive_response(connection(3), &other, &mut catalog),
            HashResponseDisposition::Mismatched
        );
        scheduler.peer_disconnected(connection(3));
        assert_eq!(scheduler.snapshot().active_attempts, 0);
    }
}

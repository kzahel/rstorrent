//! Session upload-slot runtime around the task-free scheduler.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use tokio::sync::watch;
use tokio::time::Instant;

use crate::upload_scheduler::{
    UploadGrant, UploadPeerId, UploadScheduler, UploadSchedulerConfig, UploadSchedulerPeer,
    UploadSchedulerSnapshot,
};

#[derive(Debug)]
pub(super) struct UploadCoordinator {
    started_at: Instant,
    state: Mutex<CoordinatorState>,
}

#[derive(Debug)]
struct CoordinatorState {
    next_peer: u64,
    scheduler: UploadScheduler,
    peers: BTreeMap<UploadPeerId, CoordinatorPeer>,
}

#[derive(Debug)]
struct CoordinatorPeer {
    input: UploadSchedulerPeer,
    grants: watch::Sender<UploadGrant>,
}

#[derive(Debug)]
pub(super) struct UploadMembership {
    pub id: UploadPeerId,
    pub grants: watch::Receiver<UploadGrant>,
}

impl UploadCoordinator {
    pub fn new(config: UploadSchedulerConfig) -> Result<Self, &'static str> {
        Ok(Self {
            started_at: Instant::now(),
            state: Mutex::new(CoordinatorState {
                next_peer: 1,
                scheduler: UploadScheduler::new(config)?,
                peers: BTreeMap::new(),
            }),
        })
    }

    pub fn register(&self, torrent: [u8; 20], piece_length: u32) -> UploadMembership {
        let mut state = self.state_guard();
        let id = UploadPeerId::new(state.next_peer).expect("upload peer generation is nonzero");
        state.next_peer = state.next_peer.wrapping_add(1).max(1);
        let input = UploadSchedulerPeer {
            id,
            torrent,
            piece_length,
            interested: false,
            payload_uploaded: 0,
        };
        let (grants, receiver) = watch::channel(UploadGrant::Choked);
        state
            .scheduler
            .update_peer(input, self.started_at.elapsed());
        state.peers.insert(id, CoordinatorPeer { input, grants });
        UploadMembership {
            id,
            grants: receiver,
        }
    }

    pub fn update_interest(&self, id: UploadPeerId, interested: bool) {
        let mut state = self.state_guard();
        let Some(peer) = state.peers.get_mut(&id) else {
            return;
        };
        if peer.input.interested == interested {
            return;
        }
        peer.input.interested = interested;
        let input = peer.input;
        state
            .scheduler
            .update_peer(input, self.started_at.elapsed());
        self.evaluate_locked(&mut state);
    }

    pub fn update_payload(&self, id: UploadPeerId, payload_uploaded: u64) {
        let mut state = self.state_guard();
        let Some(peer) = state.peers.get_mut(&id) else {
            return;
        };
        if payload_uploaded <= peer.input.payload_uploaded {
            return;
        }
        peer.input.payload_uploaded = payload_uploaded;
        let input = peer.input;
        state
            .scheduler
            .update_peer(input, self.started_at.elapsed());
    }

    pub fn remove(&self, id: UploadPeerId) {
        let mut state = self.state_guard();
        if state.peers.remove(&id).is_none() {
            return;
        }
        state.scheduler.remove_peer(id);
        self.evaluate_locked(&mut state);
    }

    pub fn evaluate(&self) {
        self.evaluate_locked(&mut self.state_guard());
    }

    pub fn reconfigure_slots(&self, slots: usize) {
        let mut state = self.state_guard();
        let decisions = state
            .scheduler
            .reconfigure_slots(slots, self.started_at.elapsed());
        Self::publish_decisions(&state, decisions);
    }

    pub fn snapshot(&self) -> UploadSchedulerSnapshot {
        self.state_guard().scheduler.snapshot()
    }

    fn evaluate_locked(&self, state: &mut CoordinatorState) {
        let decisions = state.scheduler.evaluate(self.started_at.elapsed());
        Self::publish_decisions(state, decisions);
    }

    fn publish_decisions(
        state: &CoordinatorState,
        decisions: Vec<crate::upload_scheduler::UploadDecision>,
    ) {
        for decision in decisions {
            if let Some(peer) = state.peers.get(&decision.peer) {
                peer.grants.send_replace(decision.grant);
            }
        }
    }

    fn state_guard(&self) -> MutexGuard<'_, CoordinatorState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::UploadCoordinator;
    use crate::upload_scheduler::{UploadGrant, UploadSchedulerConfig};

    fn coordinator(slots: usize) -> UploadCoordinator {
        UploadCoordinator::new(UploadSchedulerConfig {
            slots,
            unchoke_interval: Duration::from_secs(15),
            optimistic_interval: Duration::from_secs(30),
            seeding_piece_quota: 20,
        })
        .expect("coordinator")
    }

    #[test]
    fn interest_and_departure_publish_latest_value_grants() {
        let coordinator = coordinator(1);
        let first = coordinator.register([1; 20], 16_384);
        let second = coordinator.register([2; 20], 16_384);
        coordinator.update_interest(first.id, true);
        coordinator.update_interest(second.id, true);
        assert_eq!(*first.grants.borrow(), UploadGrant::Optimistic);
        assert_eq!(*second.grants.borrow(), UploadGrant::Choked);

        coordinator.remove(first.id);
        assert_eq!(*second.grants.borrow(), UploadGrant::Optimistic);
        assert_eq!(coordinator.snapshot().peers, 1);
    }

    #[test]
    fn reconfiguration_preserves_memberships_and_immediately_replaces_grants() {
        let coordinator = coordinator(8);
        let peers = (1..=10)
            .map(|value| coordinator.register([value; 20], 16_384))
            .collect::<Vec<_>>();
        for peer in &peers {
            coordinator.update_interest(peer.id, true);
        }
        assert_eq!(coordinator.snapshot().regular, 7);
        assert_eq!(coordinator.snapshot().optimistic, 1);

        coordinator.reconfigure_slots(0);
        assert!(
            peers
                .iter()
                .all(|peer| *peer.grants.borrow() == UploadGrant::Choked)
        );
        coordinator.reconfigure_slots(1);
        assert_eq!(coordinator.snapshot().peers, 10);
        assert_eq!(coordinator.snapshot().optimistic, 1);
        coordinator.reconfigure_slots(8);
        assert_eq!(coordinator.snapshot().regular, 7);
        assert_eq!(coordinator.snapshot().optimistic, 1);
    }
}

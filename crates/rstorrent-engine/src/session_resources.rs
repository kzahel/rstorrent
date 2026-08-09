//! Session-wide download resource authority.
//!
//! Torrent runtimes retain their local working-set bounds, while this owner
//! supplies the aggregate ceilings that must not multiply with active torrent
//! count. Registrations are generation-scoped and release any abandoned
//! accounting when the final handle is dropped.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::driver::DownloadResourceLimits;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionDownloadResourceSnapshot {
    pub outstanding_request_bytes: usize,
    pub outstanding_request_high_water: usize,
    pub buffered_payload_bytes: usize,
    pub buffered_payload_high_water: usize,
    pub active_piece_bytes: usize,
    pub active_piece_bytes_high_water: usize,
    pub active_pieces: usize,
    pub active_pieces_high_water: usize,
    pub active_storage_writes: usize,
    pub active_storage_writes_high_water: usize,
    pub active_storage_hashes: usize,
    pub active_storage_hashes_high_water: usize,
    pub active_tracker_operations: usize,
    pub active_tracker_operations_high_water: usize,
    pub registered_generations: usize,
    pub outbound_turns_granted: usize,
}

#[derive(Clone, Debug)]
pub struct SessionDownloadResources {
    inner: Arc<SessionDownloadResourcesInner>,
}

#[derive(Debug)]
struct SessionDownloadResourcesInner {
    limits: DownloadResourceLimits,
    outstanding_request_bytes: AtomicUsize,
    outstanding_request_high_water: AtomicUsize,
    buffered_payload_bytes: AtomicUsize,
    buffered_payload_high_water: AtomicUsize,
    active_piece_bytes: AtomicUsize,
    active_piece_bytes_high_water: AtomicUsize,
    active_pieces: AtomicUsize,
    active_pieces_high_water: AtomicUsize,
    storage_writes: Arc<Semaphore>,
    active_storage_writes: Arc<AtomicUsize>,
    active_storage_writes_high_water: AtomicUsize,
    storage_hashes: Arc<Semaphore>,
    active_storage_hashes: Arc<AtomicUsize>,
    active_storage_hashes_high_water: AtomicUsize,
    tracker_operations: Arc<Semaphore>,
    active_tracker_operations: Arc<AtomicUsize>,
    active_tracker_operations_high_water: AtomicUsize,
    registrations: Mutex<BTreeMap<String, Weak<TorrentResourceUsage>>>,
    dial_fairness: Mutex<DialFairness>,
    outbound_turns_granted: AtomicUsize,
}

#[derive(Debug, Default)]
struct DialFairness {
    last_granted: Option<String>,
}

#[derive(Debug)]
struct TorrentResourceUsage {
    owner: Weak<SessionDownloadResourcesInner>,
    key: String,
    outstanding_request_bytes: AtomicUsize,
    buffered_payload_bytes: AtomicUsize,
    active_piece_bytes: AtomicUsize,
    active_pieces: AtomicUsize,
}

#[derive(Clone, Debug)]
pub struct SessionTorrentResources {
    usage: Arc<TorrentResourceUsage>,
}

#[derive(Clone, Copy, Debug)]
enum ReservationKind {
    RequestBytes,
    PayloadBytes,
    ActivePiece { bytes: usize },
}

#[derive(Debug)]
pub(crate) struct SessionResourceReservation {
    resources: SessionTorrentResources,
    kind: ReservationKind,
    amount: usize,
    committed: bool,
}

impl SessionResourceReservation {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for SessionResourceReservation {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        match self.kind {
            ReservationKind::RequestBytes => self.resources.release_request_bytes(self.amount),
            ReservationKind::PayloadBytes => self.resources.release_payload_bytes(self.amount),
            ReservationKind::ActivePiece { bytes } => self.resources.release_active_piece(bytes),
        }
    }
}

#[derive(Debug)]
pub(crate) struct SessionExecutionPermit {
    _permit: OwnedSemaphorePermit,
    active: Arc<AtomicUsize>,
}

impl Drop for SessionExecutionPermit {
    fn drop(&mut self) {
        checked_release(&self.active, 1, "session execution permit");
    }
}

impl SessionDownloadResources {
    pub fn new(
        limits: DownloadResourceLimits,
        storage_write_concurrency: usize,
        storage_hash_concurrency: usize,
    ) -> Self {
        assert!(storage_write_concurrency > 0);
        assert!(storage_hash_concurrency > 0);
        Self {
            inner: Arc::new(SessionDownloadResourcesInner {
                limits,
                outstanding_request_bytes: AtomicUsize::new(0),
                outstanding_request_high_water: AtomicUsize::new(0),
                buffered_payload_bytes: AtomicUsize::new(0),
                buffered_payload_high_water: AtomicUsize::new(0),
                active_piece_bytes: AtomicUsize::new(0),
                active_piece_bytes_high_water: AtomicUsize::new(0),
                active_pieces: AtomicUsize::new(0),
                active_pieces_high_water: AtomicUsize::new(0),
                storage_writes: Arc::new(Semaphore::new(storage_write_concurrency)),
                active_storage_writes: Arc::new(AtomicUsize::new(0)),
                active_storage_writes_high_water: AtomicUsize::new(0),
                storage_hashes: Arc::new(Semaphore::new(storage_hash_concurrency)),
                active_storage_hashes: Arc::new(AtomicUsize::new(0)),
                active_storage_hashes_high_water: AtomicUsize::new(0),
                tracker_operations: Arc::new(Semaphore::new(8)),
                active_tracker_operations: Arc::new(AtomicUsize::new(0)),
                active_tracker_operations_high_water: AtomicUsize::new(0),
                registrations: Mutex::new(BTreeMap::new()),
                dial_fairness: Mutex::new(DialFairness::default()),
                outbound_turns_granted: AtomicUsize::new(0),
            }),
        }
    }

    pub fn register(&self, torrent_id: &str, generation: u64) -> SessionTorrentResources {
        let key = format!("{torrent_id}:{generation}");
        let usage = Arc::new(TorrentResourceUsage {
            owner: Arc::downgrade(&self.inner),
            key: key.clone(),
            outstanding_request_bytes: AtomicUsize::new(0),
            buffered_payload_bytes: AtomicUsize::new(0),
            active_piece_bytes: AtomicUsize::new(0),
            active_pieces: AtomicUsize::new(0),
        });
        self.inner
            .registrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, Arc::downgrade(&usage));
        SessionTorrentResources { usage }
    }

    pub fn snapshot(&self) -> SessionDownloadResourceSnapshot {
        let registered_generations = {
            let mut registrations = self
                .inner
                .registrations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            registrations.retain(|_, usage| usage.strong_count() != 0);
            registrations.len()
        };
        SessionDownloadResourceSnapshot {
            outstanding_request_bytes: load(&self.inner.outstanding_request_bytes),
            outstanding_request_high_water: load(&self.inner.outstanding_request_high_water),
            buffered_payload_bytes: load(&self.inner.buffered_payload_bytes),
            buffered_payload_high_water: load(&self.inner.buffered_payload_high_water),
            active_piece_bytes: load(&self.inner.active_piece_bytes),
            active_piece_bytes_high_water: load(&self.inner.active_piece_bytes_high_water),
            active_pieces: load(&self.inner.active_pieces),
            active_pieces_high_water: load(&self.inner.active_pieces_high_water),
            active_storage_writes: load(&self.inner.active_storage_writes),
            active_storage_writes_high_water: load(&self.inner.active_storage_writes_high_water),
            active_storage_hashes: load(&self.inner.active_storage_hashes),
            active_storage_hashes_high_water: load(&self.inner.active_storage_hashes_high_water),
            active_tracker_operations: load(&self.inner.active_tracker_operations),
            active_tracker_operations_high_water: load(
                &self.inner.active_tracker_operations_high_water,
            ),
            registered_generations,
            outbound_turns_granted: load(&self.inner.outbound_turns_granted),
        }
    }
}

impl SessionTorrentResources {
    fn owner(&self) -> Arc<SessionDownloadResourcesInner> {
        self.usage
            .owner
            .upgrade()
            .expect("session resources outlive torrent registrations")
    }

    pub(crate) fn try_reserve_request_bytes(
        &self,
        bytes: usize,
    ) -> Option<SessionResourceReservation> {
        let owner = self.owner();
        try_reserve(
            &owner.outstanding_request_bytes,
            &owner.outstanding_request_high_water,
            owner.limits.max_outstanding_request_bytes,
            bytes,
        )?;
        self.usage
            .outstanding_request_bytes
            .fetch_add(bytes, Ordering::AcqRel);
        Some(SessionResourceReservation {
            resources: self.clone(),
            kind: ReservationKind::RequestBytes,
            amount: bytes,
            committed: false,
        })
    }

    pub(crate) fn can_reserve_request_bytes(&self, bytes: usize) -> bool {
        let owner = self.owner();
        load(&owner.outstanding_request_bytes)
            .checked_add(bytes)
            .is_some_and(|next| next <= owner.limits.max_outstanding_request_bytes)
    }

    pub(crate) fn release_request_bytes(&self, bytes: usize) {
        let owner = self.owner();
        checked_release(
            &self.usage.outstanding_request_bytes,
            bytes,
            "torrent request bytes",
        );
        checked_release(
            &owner.outstanding_request_bytes,
            bytes,
            "session request bytes",
        );
    }

    pub(crate) fn try_reserve_payload_bytes(
        &self,
        bytes: usize,
    ) -> Option<SessionResourceReservation> {
        let owner = self.owner();
        try_reserve(
            &owner.buffered_payload_bytes,
            &owner.buffered_payload_high_water,
            owner.limits.max_buffered_payload_bytes,
            bytes,
        )?;
        self.usage
            .buffered_payload_bytes
            .fetch_add(bytes, Ordering::AcqRel);
        Some(SessionResourceReservation {
            resources: self.clone(),
            kind: ReservationKind::PayloadBytes,
            amount: bytes,
            committed: false,
        })
    }

    pub(crate) fn release_payload_bytes(&self, bytes: usize) {
        let owner = self.owner();
        checked_release(
            &self.usage.buffered_payload_bytes,
            bytes,
            "torrent payload bytes",
        );
        checked_release(
            &owner.buffered_payload_bytes,
            bytes,
            "session payload bytes",
        );
    }

    pub(crate) fn can_reserve_active_piece(&self, bytes: usize) -> bool {
        let owner = self.owner();
        load(&owner.active_piece_bytes)
            .checked_add(bytes)
            .is_some_and(|next| next <= owner.limits.max_active_piece_bytes)
            && load(&owner.active_pieces) < owner.limits.max_active_pieces
    }

    pub(crate) fn try_reserve_active_piece(
        &self,
        bytes: usize,
    ) -> Option<SessionResourceReservation> {
        let owner = self.owner();
        try_reserve(
            &owner.active_piece_bytes,
            &owner.active_piece_bytes_high_water,
            owner.limits.max_active_piece_bytes,
            bytes,
        )?;
        if try_reserve(
            &owner.active_pieces,
            &owner.active_pieces_high_water,
            owner.limits.max_active_pieces,
            1,
        )
        .is_none()
        {
            checked_release(
                &owner.active_piece_bytes,
                bytes,
                "session active piece bytes",
            );
            return None;
        }
        self.usage
            .active_piece_bytes
            .fetch_add(bytes, Ordering::AcqRel);
        self.usage.active_pieces.fetch_add(1, Ordering::AcqRel);
        Some(SessionResourceReservation {
            resources: self.clone(),
            kind: ReservationKind::ActivePiece { bytes },
            amount: 1,
            committed: false,
        })
    }

    pub(crate) fn release_active_piece(&self, bytes: usize) {
        let owner = self.owner();
        checked_release(
            &self.usage.active_piece_bytes,
            bytes,
            "torrent active piece bytes",
        );
        checked_release(&self.usage.active_pieces, 1, "torrent active pieces");
        checked_release(
            &owner.active_piece_bytes,
            bytes,
            "session active piece bytes",
        );
        checked_release(&owner.active_pieces, 1, "session active pieces");
    }

    pub(crate) fn try_acquire_outbound_turn(&self) -> bool {
        let owner = self.owner();
        let mut fairness = owner
            .dial_fairness
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if fairness.last_granted.as_deref() == Some(self.usage.key.as_str()) {
            fairness.last_granted = None;
            return false;
        }
        fairness.last_granted = Some(self.usage.key.clone());
        owner.outbound_turns_granted.fetch_add(1, Ordering::AcqRel);
        true
    }

    pub(crate) async fn acquire_storage_write(&self) -> SessionExecutionPermit {
        let owner = self.owner();
        execution_permit(
            owner.storage_writes.clone(),
            owner.active_storage_writes.clone(),
            &owner.active_storage_writes_high_water,
        )
        .await
    }

    pub(crate) async fn acquire_storage_hash(&self) -> SessionExecutionPermit {
        let owner = self.owner();
        execution_permit(
            owner.storage_hashes.clone(),
            owner.active_storage_hashes.clone(),
            &owner.active_storage_hashes_high_water,
        )
        .await
    }

    pub(crate) async fn acquire_tracker_operation(&self) -> SessionExecutionPermit {
        let owner = self.owner();
        execution_permit(
            owner.tracker_operations.clone(),
            owner.active_tracker_operations.clone(),
            &owner.active_tracker_operations_high_water,
        )
        .await
    }
}

impl Drop for TorrentResourceUsage {
    fn drop(&mut self) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let request = self.outstanding_request_bytes.swap(0, Ordering::AcqRel);
        let payload = self.buffered_payload_bytes.swap(0, Ordering::AcqRel);
        let active_bytes = self.active_piece_bytes.swap(0, Ordering::AcqRel);
        let active_pieces = self.active_pieces.swap(0, Ordering::AcqRel);
        checked_release(
            &owner.outstanding_request_bytes,
            request,
            "session request bytes",
        );
        checked_release(
            &owner.buffered_payload_bytes,
            payload,
            "session payload bytes",
        );
        checked_release(
            &owner.active_piece_bytes,
            active_bytes,
            "session active piece bytes",
        );
        checked_release(&owner.active_pieces, active_pieces, "session active pieces");
        owner
            .registrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
    }
}

async fn execution_permit(
    semaphore: Arc<Semaphore>,
    active: Arc<AtomicUsize>,
    high_water: &AtomicUsize,
) -> SessionExecutionPermit {
    let permit = semaphore
        .acquire_owned()
        .await
        .expect("session resource semaphore remains open");
    let current = active.fetch_add(1, Ordering::AcqRel).saturating_add(1);
    high_water.fetch_max(current, Ordering::AcqRel);
    SessionExecutionPermit {
        _permit: permit,
        active,
    }
}

fn try_reserve(
    counter: &AtomicUsize,
    high_water: &AtomicUsize,
    limit: usize,
    amount: usize,
) -> Option<()> {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        let next = current.checked_add(amount)?;
        if next > limit {
            return None;
        }
        match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                high_water.fetch_max(next, Ordering::AcqRel);
                return Some(());
            }
            Err(actual) => current = actual,
        }
    }
}

fn checked_release(counter: &AtomicUsize, amount: usize, owner: &str) {
    if amount == 0 {
        return;
    }
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_sub(amount)
        })
        .unwrap_or_else(|_| panic!("{owner} accounting underflow"));
}

fn load(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::SessionDownloadResources;
    use crate::DownloadResourceLimits;

    fn resources() -> SessionDownloadResources {
        SessionDownloadResources::new(
            DownloadResourceLimits {
                max_outstanding_request_bytes: 32,
                max_buffered_payload_bytes: 16,
                max_active_piece_bytes: 64,
                max_active_pieces: 2,
            },
            1,
            1,
        )
    }

    #[test]
    fn generation_registrations_share_hard_memory_ceilings_and_release_once() {
        let resources = resources();
        let first = resources.register("first", 1);
        let second = resources.register("second", 1);

        first
            .try_reserve_request_bytes(24)
            .expect("first request reservation")
            .commit();
        assert!(second.try_reserve_request_bytes(16).is_none());
        second
            .try_reserve_request_bytes(8)
            .expect("remaining request reservation")
            .commit();
        first
            .try_reserve_payload_bytes(16)
            .expect("payload reservation")
            .commit();
        assert!(second.try_reserve_payload_bytes(1).is_none());
        first
            .try_reserve_active_piece(48)
            .expect("first piece")
            .commit();
        second
            .try_reserve_active_piece(16)
            .expect("second piece")
            .commit();
        assert!(second.try_reserve_active_piece(1).is_none());

        let full = resources.snapshot();
        assert_eq!(full.outstanding_request_bytes, 32);
        assert_eq!(full.buffered_payload_bytes, 16);
        assert_eq!(full.active_piece_bytes, 64);
        assert_eq!(full.active_pieces, 2);
        assert_eq!(full.registered_generations, 2);

        first.release_request_bytes(24);
        second.release_request_bytes(8);
        first.release_payload_bytes(16);
        first.release_active_piece(48);
        second.release_active_piece(16);
        drop(first);
        drop(second);
        let released = resources.snapshot();
        assert_eq!(released.outstanding_request_bytes, 0);
        assert_eq!(released.buffered_payload_bytes, 0);
        assert_eq!(released.active_piece_bytes, 0);
        assert_eq!(released.active_pieces, 0);
        assert_eq!(released.registered_generations, 0);
    }

    #[test]
    fn final_registration_drop_recovers_abandoned_accounting() {
        let resources = resources();
        let registration = resources.register("abandoned", 9);
        registration
            .try_reserve_request_bytes(32)
            .expect("request reservation")
            .commit();
        registration
            .try_reserve_payload_bytes(16)
            .expect("payload reservation")
            .commit();
        registration
            .try_reserve_active_piece(64)
            .expect("piece reservation")
            .commit();
        drop(registration);
        let snapshot = resources.snapshot();
        assert_eq!(snapshot.outstanding_request_bytes, 0);
        assert_eq!(snapshot.buffered_payload_bytes, 0);
        assert_eq!(snapshot.active_piece_bytes, 0);
        assert_eq!(snapshot.active_pieces, 0);
    }

    #[tokio::test]
    async fn storage_and_tracker_execution_are_session_bounded() {
        let resources = resources();
        let first = resources.register("first", 1);
        let write = first.acquire_storage_write().await;
        let hash = first.acquire_storage_hash().await;
        let tracker = first.acquire_tracker_operation().await;
        let active = resources.snapshot();
        assert_eq!(active.active_storage_writes, 1);
        assert_eq!(active.active_storage_hashes, 1);
        assert_eq!(active.active_tracker_operations, 1);
        drop(write);
        drop(hash);
        drop(tracker);
        let released = resources.snapshot();
        assert_eq!(released.active_storage_writes, 0);
        assert_eq!(released.active_storage_hashes, 0);
        assert_eq!(released.active_tracker_operations, 0);
        assert_eq!(released.active_storage_writes_high_water, 1);
        assert_eq!(released.active_storage_hashes_high_water, 1);
        assert_eq!(released.active_tracker_operations_high_water, 1);
    }

    #[test]
    fn outbound_turns_yield_between_repeated_requests() {
        let resources = resources();
        let first = resources.register("first", 1);
        let second = resources.register("second", 1);
        assert!(first.try_acquire_outbound_turn());
        assert!(!first.try_acquire_outbound_turn());
        assert!(second.try_acquire_outbound_turn());
        assert!(first.try_acquire_outbound_turn());
        assert_eq!(resources.snapshot().outbound_turns_granted, 3);
    }
}

//! Application-generation lifetime for one torrent's peer state and children.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rstorrent_engine::peer::PeerRegistrySnapshot;
use rstorrent_engine::{
    DownloadControl, PeerConnectionObservation, SeedRegistrationToken, StorageFilePool,
    TorrentPeerActivitySink, TorrentPeerError, TorrentPeerHandle,
};
use tokio::task::JoinHandle;

use crate::incoming_seeding::{
    IncomingSeeding, IncomingSeedingError, SeedReconcileInput, SeedReconcileOutcome,
    SeedReconcileResult,
};
use crate::store::{ResumeRecord, StorageRootLocation};
use crate::views::ViewHub;

#[derive(Debug)]
struct TorrentPeerViewSink {
    torrent_id: String,
    accepting: Arc<AtomicBool>,
    views: ViewHub,
}

impl TorrentPeerActivitySink for TorrentPeerViewSink {
    fn record_peer_connections(
        &self,
        captured_at: Duration,
        peers: Vec<PeerConnectionObservation>,
    ) {
        if self.accepting.load(Ordering::Acquire) {
            let _ =
                self.views
                    .record_peer_connections(&self.torrent_id, captured_at, peers.as_slice());
        }
    }

    fn record_peer_registry(&self, active: bool, snapshot: PeerRegistrySnapshot) {
        if self.accepting.load(Ordering::Acquire) {
            let _ = self
                .views
                .record_peer_registry_state(&self.torrent_id, active, &snapshot);
        }
    }
}

#[derive(Debug)]
pub(crate) struct ActiveDownload {
    pub(crate) control: DownloadControl,
    pub(crate) task: JoinHandle<Result<(), String>>,
}

#[derive(Debug, Default)]
struct SeedRegistrationState {
    transition_generation: u64,
    token: Option<SeedRegistrationToken>,
}

#[derive(Clone, Debug)]
pub(crate) struct TorrentRuntimeHandle {
    peers: TorrentPeerHandle,
    seed_registration: Arc<Mutex<SeedRegistrationState>>,
}

impl TorrentRuntimeHandle {
    pub(crate) async fn reconcile_seed(
        &self,
        incoming: &IncomingSeeding,
        resume: &ResumeRecord,
        catalog_eligible: bool,
        root: Option<&StorageRootLocation>,
        active_download: bool,
        storage_file_pool: &StorageFilePool,
    ) -> Result<Option<SeedReconcileOutcome>, TorrentRuntimeError> {
        let (generation, current) = self.begin_seed_transition()?;
        let result = incoming
            .reconcile(SeedReconcileInput {
                resume,
                catalog_eligible,
                root,
                active_download,
                current,
                torrent_peers: self.peers.clone(),
                storage_file_pool,
            })
            .await;
        match result {
            Ok(result) => {
                self.finish_seed_transition(incoming, generation, result, active_download)
                    .await
            }
            Err(error) => {
                self.restore_failed_seed_transition(incoming, generation, current)
                    .await?;
                Err(error.into())
            }
        }
    }

    pub(crate) async fn unregister_seed(
        &self,
        incoming: &IncomingSeeding,
    ) -> Result<(), TorrentRuntimeError> {
        let token = {
            let mut state = self.seed_state();
            state.transition_generation = state
                .transition_generation
                .checked_add(1)
                .ok_or(TorrentRuntimeError::RegistrationGenerationExhausted)?;
            state.token.take()
        };
        if let Some(token) = token {
            incoming.unregister(token).await?;
        }
        Ok(())
    }

    pub(crate) fn has_seed_registration(&self) -> bool {
        self.seed_state().token.is_some()
    }

    pub(crate) fn forget_seed_registration(&self) -> Result<(), TorrentRuntimeError> {
        let mut state = self.seed_state();
        state.transition_generation = state
            .transition_generation
            .checked_add(1)
            .ok_or(TorrentRuntimeError::RegistrationGenerationExhausted)?;
        state.token = None;
        Ok(())
    }

    pub(crate) fn publish_inactive(&self) -> Result<(), TorrentRuntimeError> {
        self.peers.publish_inactive()?;
        Ok(())
    }

    fn begin_seed_transition(
        &self,
    ) -> Result<(u64, Option<SeedRegistrationToken>), TorrentRuntimeError> {
        let mut state = self.seed_state();
        state.transition_generation = state
            .transition_generation
            .checked_add(1)
            .ok_or(TorrentRuntimeError::RegistrationGenerationExhausted)?;
        let generation = state.transition_generation;
        Ok((generation, state.token.take()))
    }

    async fn finish_seed_transition(
        &self,
        incoming: &IncomingSeeding,
        generation: u64,
        result: SeedReconcileResult,
        active_download: bool,
    ) -> Result<Option<SeedReconcileOutcome>, TorrentRuntimeError> {
        let (committed, stale_token, active) = {
            let mut state = self.seed_state();
            if state.transition_generation == generation {
                state.token = result.token;
                (true, None, active_download || state.token.is_some())
            } else {
                (false, result.token, state.token.is_some())
            }
        };
        if let Some(token) = stale_token {
            incoming.unregister(token).await?;
        }
        if committed {
            if active {
                self.peers.publish_active(true)?;
            } else {
                self.peers.publish_inactive()?;
            }
            Ok(Some(result.outcome))
        } else {
            Ok(None)
        }
    }

    async fn restore_failed_seed_transition(
        &self,
        incoming: &IncomingSeeding,
        generation: u64,
        current: Option<SeedRegistrationToken>,
    ) -> Result<(), TorrentRuntimeError> {
        let stale_token = {
            let mut state = self.seed_state();
            if state.transition_generation == generation {
                state.token = current;
                None
            } else {
                current
            }
        };
        if let Some(token) = stale_token {
            incoming.unregister(token).await?;
        }
        Ok(())
    }

    fn seed_state(&self) -> std::sync::MutexGuard<'_, SeedRegistrationState> {
        self.seed_registration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug)]
pub(crate) enum TorrentRuntimeError {
    Incoming(IncomingSeedingError),
    Peer(TorrentPeerError),
    RegistrationGenerationExhausted,
}

impl std::fmt::Display for TorrentRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incoming(error) => write!(formatter, "{error}"),
            Self::Peer(error) => write!(formatter, "{error}"),
            Self::RegistrationGenerationExhausted => {
                formatter.write_str("seed registration transition generation exhausted")
            }
        }
    }
}

impl std::error::Error for TorrentRuntimeError {}

impl From<IncomingSeedingError> for TorrentRuntimeError {
    fn from(error: IncomingSeedingError) -> Self {
        Self::Incoming(error)
    }
}

impl From<TorrentPeerError> for TorrentRuntimeError {
    fn from(error: TorrentPeerError) -> Self {
        Self::Peer(error)
    }
}

#[derive(Debug)]
pub(crate) struct TorrentRuntime {
    torrent_id: String,
    generation: u64,
    accepting_peer_events: Arc<AtomicBool>,
    peers: TorrentPeerHandle,
    handle: TorrentRuntimeHandle,
    active_download: Option<ActiveDownload>,
}

impl TorrentRuntime {
    pub(crate) fn new(
        torrent_id: String,
        generation: u64,
        views: ViewHub,
    ) -> Result<Self, TorrentPeerError> {
        let accepting_peer_events = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(TorrentPeerViewSink {
            torrent_id: torrent_id.clone(),
            accepting: accepting_peer_events.clone(),
            views,
        });
        let peers = TorrentPeerHandle::new(sink)?;
        let handle = TorrentRuntimeHandle {
            peers: peers.clone(),
            seed_registration: Arc::new(Mutex::new(SeedRegistrationState::default())),
        };
        Ok(Self {
            torrent_id,
            generation,
            accepting_peer_events,
            peers,
            handle,
            active_download: None,
        })
    }

    pub(crate) fn torrent_id(&self) -> &str {
        &self.torrent_id
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn peers(&self) -> TorrentPeerHandle {
        debug_assert_ne!(self.generation, 0);
        self.peers.clone()
    }

    pub(crate) fn handle(&self) -> TorrentRuntimeHandle {
        self.handle.clone()
    }

    pub(crate) fn active_download(&self) -> Option<&ActiveDownload> {
        self.active_download.as_ref()
    }

    pub(crate) fn set_active_download(&mut self, active: ActiveDownload) {
        debug_assert!(self.active_download.is_none());
        self.active_download = Some(active);
    }

    pub(crate) fn take_active_download(&mut self) -> Option<ActiveDownload> {
        self.active_download.take()
    }

    pub(crate) fn publish_inactive(&self) -> Result<(), TorrentPeerError> {
        self.peers.publish_inactive()
    }

    pub(crate) fn deactivate_peer_events(&self) {
        self.accepting_peer_events.store(false, Ordering::Release);
    }
}

impl Drop for TorrentRuntime {
    fn drop(&mut self) {
        if let Some(active) = &self.active_download {
            active.control.cancel();
        }
        self.accepting_peer_events.store(false, Ordering::Release);
    }
}

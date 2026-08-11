//! Per-torrent peer state and connection cancellation shared by socket owners.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_protocol::mse::MseMethod;
use rstorrent_protocol::peer_wire::PeerMessage;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::bandwidth::{TorrentBandwidth, TorrentTransferRateLimits};
use crate::network::{AddressFamilyPolicy, AddressFamilyPolicyHandle};
use crate::peer::{
    DialAttempt, DialCandidate, DialEligibility, PeerFailure, PeerObservation, PeerRecordId,
    PeerRegistry, PeerRegistryConfig, PeerRegistryCounts, PeerRegistryError, PeerRegistrySnapshot,
    PeerSelectionContext, PeerSource,
};
use crate::peer_runtime::{
    IncomingPeerStart, PeerAdmissionOutcome, PeerConnectionObservation, PeerConnectionRole,
    PeerRuntime, PeerRuntimeError, PeerTransport, PeerUploadActivity,
};
use crate::pex::{PexError, PexState};
use crate::swarm::ConnectionId;

const PEER_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
pub(crate) const INCOMING_CONTENT_EVENT_CAPACITY: usize = 64;
pub(crate) const INCOMING_CONTENT_COMMAND_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IncomingContentCapabilities {
    pub(crate) fast: bool,
}

#[derive(Debug)]
pub(crate) enum IncomingContentCommand {
    Send(PeerMessage),
}

#[derive(Debug)]
pub(crate) enum IncomingContentEvent {
    Connected {
        attachment: IncomingPeerAttachment,
        capabilities: IncomingContentCapabilities,
        commands: mpsc::Sender<IncomingContentCommand>,
    },
    Message {
        attachment: IncomingPeerAttachment,
        message: PeerMessage,
    },
    Stopped {
        attachment: IncomingPeerAttachment,
        failure: Option<PeerFailure>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IncomingContentRouteToken(u64);

#[derive(Debug, Default)]
struct IncomingContentRouteState {
    next_generation: u64,
    active: Option<(
        IncomingContentRouteToken,
        mpsc::Sender<IncomingContentEvent>,
    )>,
}

pub trait TorrentPeerActivitySink: Send + Sync + fmt::Debug {
    fn record_peer_connections(&self, captured_at: Duration, peers: Vec<PeerConnectionObservation>);

    fn record_peer_registry(&self, active: bool, snapshot: PeerRegistrySnapshot);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IncomingPeerAttachment {
    connection_id: ConnectionId,
    record_id: PeerRecordId,
}

impl IncomingPeerAttachment {
    pub fn connection_id(self) -> ConnectionId {
        self.connection_id
    }

    pub fn record_id(self) -> PeerRecordId {
        self.record_id
    }
}

#[derive(Debug)]
pub enum TorrentPeerError {
    Registry(PeerRegistryError),
    Runtime(PeerRuntimeError),
    Pex(PexError),
    AddressFamilyDenied(SocketAddr),
    ConnectionIdentifierOverflow,
}

impl fmt::Display for TorrentPeerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(error) => write!(formatter, "peer registry: {error}"),
            Self::Runtime(error) => write!(formatter, "peer runtime: {error}"),
            Self::Pex(error) => write!(formatter, "peer exchange: {error}"),
            Self::AddressFamilyDenied(address) => {
                write!(formatter, "peer address family is disabled: {address}")
            }
            Self::ConnectionIdentifierOverflow => {
                formatter.write_str("peer connection identifier overflow")
            }
        }
    }
}

impl Error for TorrentPeerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Registry(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::Pex(error) => Some(error),
            Self::AddressFamilyDenied(_) | Self::ConnectionIdentifierOverflow => None,
        }
    }
}

impl From<PeerRegistryError> for TorrentPeerError {
    fn from(error: PeerRegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<PeerRuntimeError> for TorrentPeerError {
    fn from(error: PeerRuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<PexError> for TorrentPeerError {
    fn from(error: PexError) -> Self {
        Self::Pex(error)
    }
}

#[derive(Debug, Default)]
struct RegistryPublicationState {
    active: bool,
    last_emitted: Option<PeerRegistrySnapshot>,
    next_transition_at: Option<Duration>,
}

#[derive(Debug)]
pub(crate) struct TorrentPeerState {
    pub(crate) registry: PeerRegistry,
    pub(crate) runtime: PeerRuntime,
    pub(crate) pex: PexState,
    next_connection_id: u64,
    last_connections_emitted: Vec<PeerConnectionObservation>,
    last_connections_emitted_at: Option<Duration>,
    registry_publication: RegistryPublicationState,
}

impl TorrentPeerState {
    fn new(config: PeerRegistryConfig) -> Result<Self, TorrentPeerError> {
        Ok(Self {
            registry: PeerRegistry::new(config)?,
            runtime: PeerRuntime::default(),
            pex: PexState::default(),
            next_connection_id: 1,
            last_connections_emitted: Vec::new(),
            last_connections_emitted_at: None,
            registry_publication: RegistryPublicationState::default(),
        })
    }

    pub(crate) fn begin_dial(
        &mut self,
        candidate: DialCandidate,
        role: PeerConnectionRole,
        now: Duration,
    ) -> Result<DialAttempt, TorrentPeerError> {
        let connection_id = self.allocate_connection_id()?;
        let context = PeerSelectionContext { now };
        let attempt =
            self.registry
                .begin_dial_with_connection_id(candidate, context, connection_id)?;
        if let Err(error) = self.runtime.begin_outgoing(attempt, role, now) {
            let _ = self.registry.dial_cancelled(attempt);
            return Err(error.into());
        }
        Ok(attempt)
    }

    fn begin_incoming(
        &mut self,
        start: TorrentIncomingStart,
    ) -> Result<IncomingPeerAttachment, TorrentPeerError> {
        let TorrentIncomingStart {
            remote_endpoint,
            local_endpoint,
            peer_id,
            supports_extensions,
            transport,
            mse_method,
            role,
            now,
        } = start;
        let connection_id = self.allocate_connection_id()?;
        let endpoint = crate::peer::PeerEndpoint::new(remote_endpoint)?;
        let observed = self.registry.observe(
            PeerObservation::new(endpoint, PeerSource::Incoming, false),
            now,
        )?;
        self.registry.incoming_connected(observed.record_id, now)?;
        if let Err(error) = self.runtime.begin_incoming(
            connection_id,
            IncomingPeerStart {
                record_id: observed.record_id,
                endpoint: remote_endpoint,
                local_endpoint,
                transport,
                role,
                peer_id,
                supports_extensions,
                mse_method,
            },
            now,
        ) {
            let _ = self.registry.incoming_closed(observed.record_id, now, None);
            return Err(error.into());
        }
        Ok(IncomingPeerAttachment {
            connection_id,
            record_id: observed.record_id,
        })
    }

    fn incoming_handshake_completed(
        &mut self,
        attachment: IncomingPeerAttachment,
        local_peer_id: [u8; 20],
        now: Duration,
    ) -> Result<PeerAdmissionOutcome, TorrentPeerError> {
        Ok(self.runtime.incoming_handshake_completed(
            attachment.connection_id,
            local_peer_id,
            now,
        )?)
    }

    fn set_incoming_upload(
        &mut self,
        attachment: IncomingPeerAttachment,
        activity: PeerUploadActivity,
    ) -> Result<bool, TorrentPeerError> {
        let peer = self.runtime.observation(attachment.connection_id).ok_or(
            PeerRuntimeError::UnknownConnection(attachment.connection_id),
        )?;
        if peer.record_id != Some(attachment.record_id) {
            return Err(PeerRuntimeError::UnknownConnection(attachment.connection_id).into());
        }
        let publish_immediately = peer.upload.is_none_or(|previous| {
            previous.interested != activity.interested || previous.grant != activity.grant
        });
        self.runtime
            .set_upload_activity(attachment.connection_id, activity)?;
        Ok(publish_immediately)
    }

    fn set_incoming_metadata_extension(
        &mut self,
        attachment: IncomingPeerAttachment,
        supported: bool,
    ) -> Result<(), TorrentPeerError> {
        self.runtime
            .set_metadata_extension(attachment.connection_id, supported)?;
        Ok(())
    }

    fn begin_incoming_disconnect(
        &mut self,
        attachment: IncomingPeerAttachment,
        failure: Option<PeerFailure>,
        now: Duration,
    ) -> Result<(), TorrentPeerError> {
        self.runtime
            .begin_disconnect(attachment.connection_id, failure, now)?;
        Ok(())
    }

    fn remove_incoming(
        &mut self,
        attachment: IncomingPeerAttachment,
        failure: Option<PeerFailure>,
        now: Duration,
    ) -> Result<(), TorrentPeerError> {
        let peer = self.runtime.observation(attachment.connection_id).ok_or(
            PeerRuntimeError::UnknownConnection(attachment.connection_id),
        )?;
        if peer.record_id != Some(attachment.record_id) {
            return Err(PeerRuntimeError::UnknownConnection(attachment.connection_id).into());
        }
        let remote_endpoint = peer.endpoint;
        let advertised = self
            .pex
            .extension_map(attachment.connection_id)
            .listen_port()
            .and_then(|port| {
                crate::peer::PeerEndpoint::new(SocketAddr::new(remote_endpoint.ip(), port)).ok()
            });
        self.pex
            .remove_source(attachment.connection_id, &mut self.registry);
        if let Some(endpoint) = advertised {
            self.pex.peer_dropped(endpoint);
        }
        self.runtime.remove(attachment.connection_id)?;
        self.registry
            .incoming_closed(attachment.record_id, now, failure)?;
        Ok(())
    }

    fn allocate_connection_id(&mut self) -> Result<ConnectionId, TorrentPeerError> {
        let connection_id = ConnectionId::new(self.next_connection_id)
            .ok_or(TorrentPeerError::ConnectionIdentifierOverflow)?;
        self.next_connection_id = self
            .next_connection_id
            .checked_add(1)
            .ok_or(TorrentPeerError::ConnectionIdentifierOverflow)?;
        Ok(connection_id)
    }

    fn refresh_sources(&mut self) -> Result<(), TorrentPeerError> {
        let connections = self
            .runtime
            .snapshot()
            .into_iter()
            .map(|peer| peer.connection_id)
            .collect::<Vec<_>>();
        for connection in connections {
            let record_id = self
                .runtime
                .observation(connection)
                .and_then(|peer| peer.record_id);
            if let Some(sources) = record_id
                .and_then(|record_id| self.registry.get(record_id))
                .map(|record| record.sources())
            {
                self.runtime.set_sources(connection, sources)?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn set_next_connection_id(&mut self, next: u64) {
        self.next_connection_id = next;
    }
}

struct TorrentIncomingStart {
    remote_endpoint: SocketAddr,
    local_endpoint: SocketAddr,
    peer_id: [u8; 20],
    supports_extensions: bool,
    transport: PeerTransport,
    mse_method: Option<MseMethod>,
    role: PeerConnectionRole,
    now: Duration,
}

#[derive(Debug)]
struct TorrentPeerHandleInner {
    started_at: Instant,
    state: Mutex<TorrentPeerState>,
    connection_cancellations: Mutex<BTreeMap<ConnectionId, CancellationToken>>,
    address_families: AddressFamilyPolicyHandle,
    sink: Mutex<Arc<dyn TorrentPeerActivitySink>>,
    incoming_content: Mutex<IncomingContentRouteState>,
    bandwidth: Mutex<Option<TorrentBandwidth>>,
}

#[derive(Clone, Debug)]
pub struct TorrentPeerHandle {
    inner: Arc<TorrentPeerHandleInner>,
}

impl TorrentPeerHandle {
    pub(crate) fn install_incoming_content_route(
        &self,
        sender: mpsc::Sender<IncomingContentEvent>,
    ) -> IncomingContentRouteToken {
        let mut route = self
            .inner
            .incoming_content
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        route.next_generation = route.next_generation.wrapping_add(1).max(1);
        let token = IncomingContentRouteToken(route.next_generation);
        route.active = Some((token, sender));
        token
    }

    pub(crate) fn remove_incoming_content_route(&self, token: IncomingContentRouteToken) {
        let mut route = self
            .inner
            .incoming_content
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if route
            .active
            .as_ref()
            .is_some_and(|(active, _)| *active == token)
        {
            route.active = None;
        }
    }

    pub(crate) fn incoming_content_route(&self) -> Option<mpsc::Sender<IncomingContentEvent>> {
        self.inner
            .incoming_content
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active
            .as_ref()
            .map(|(_, sender)| sender.clone())
    }

    pub fn new(sink: Arc<dyn TorrentPeerActivitySink>) -> Result<Self, TorrentPeerError> {
        Self::with_registry_config(PeerRegistryConfig::default(), sink)
    }

    pub fn with_registry_config(
        config: PeerRegistryConfig,
        sink: Arc<dyn TorrentPeerActivitySink>,
    ) -> Result<Self, TorrentPeerError> {
        Ok(Self {
            inner: Arc::new(TorrentPeerHandleInner {
                started_at: Instant::now(),
                state: Mutex::new(TorrentPeerState::new(config)?),
                connection_cancellations: Mutex::new(BTreeMap::new()),
                address_families: AddressFamilyPolicyHandle::default(),
                sink: Mutex::new(sink),
                incoming_content: Mutex::new(IncomingContentRouteState::default()),
                bandwidth: Mutex::new(None),
            }),
        })
    }

    pub fn install_bandwidth(&self, bandwidth: TorrentBandwidth) {
        *self
            .inner
            .bandwidth
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(bandwidth);
    }

    pub(crate) fn bandwidth(&self) -> Option<TorrentBandwidth> {
        self.inner
            .bandwidth
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn transfer_rate_limits(&self) -> TorrentTransferRateLimits {
        self.bandwidth()
            .map_or_else(TorrentTransferRateLimits::default, |bandwidth| {
                bandwidth.limits()
            })
    }

    pub fn set_transfer_rate_limits(&self, limits: TorrentTransferRateLimits) {
        if let Some(bandwidth) = self.bandwidth() {
            bandwidth.set_limits(limits);
        }
    }

    pub(crate) fn download_rate_limited(&self) -> bool {
        self.bandwidth()
            .is_some_and(|bandwidth| bandwidth.download_limited())
    }

    pub fn elapsed(&self) -> Duration {
        self.inner.started_at.elapsed()
    }

    pub fn begin_incoming(
        &self,
        remote_endpoint: SocketAddr,
        local_endpoint: SocketAddr,
        peer_id: [u8; 20],
        supports_extensions: bool,
        role: PeerConnectionRole,
    ) -> Result<IncomingPeerAttachment, TorrentPeerError> {
        self.begin_incoming_with_mse(
            remote_endpoint,
            local_endpoint,
            peer_id,
            supports_extensions,
            role,
            None,
        )
    }

    pub(crate) fn begin_incoming_with_mse(
        &self,
        remote_endpoint: SocketAddr,
        local_endpoint: SocketAddr,
        peer_id: [u8; 20],
        supports_extensions: bool,
        role: PeerConnectionRole,
        mse_method: Option<MseMethod>,
    ) -> Result<IncomingPeerAttachment, TorrentPeerError> {
        self.begin_incoming_with_transport(
            remote_endpoint,
            local_endpoint,
            peer_id,
            supports_extensions,
            role,
            PeerTransport::Tcp,
            mse_method,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_incoming_with_transport(
        &self,
        remote_endpoint: SocketAddr,
        local_endpoint: SocketAddr,
        peer_id: [u8; 20],
        supports_extensions: bool,
        role: PeerConnectionRole,
        transport: PeerTransport,
        mse_method: Option<MseMethod>,
    ) -> Result<IncomingPeerAttachment, TorrentPeerError> {
        if !self.address_family_policy().permits(remote_endpoint.ip()) {
            return Err(TorrentPeerError::AddressFamilyDenied(remote_endpoint));
        }
        let now = self.elapsed();
        let attachment = self.with_state(|state| {
            state.begin_incoming(TorrentIncomingStart {
                remote_endpoint,
                local_endpoint,
                peer_id,
                supports_extensions,
                transport,
                mse_method,
                role,
                now,
            })
        })?;
        self.publish(true, true)?;
        Ok(attachment)
    }

    pub(crate) fn incoming_handshake_completed(
        &self,
        attachment: IncomingPeerAttachment,
        local_peer_id: [u8; 20],
    ) -> Result<PeerAdmissionOutcome, TorrentPeerError> {
        let now = self.elapsed();
        let outcome = self.with_state(|state| {
            state.incoming_handshake_completed(attachment, local_peer_id, now)
        })?;
        self.apply_admission(attachment.connection_id, outcome);
        self.publish(true, true)?;
        Ok(outcome)
    }

    pub fn set_incoming_upload(
        &self,
        attachment: IncomingPeerAttachment,
        activity: PeerUploadActivity,
    ) -> Result<(), TorrentPeerError> {
        let force = self.with_state(|state| state.set_incoming_upload(attachment, activity))?;
        self.publish(true, force)
    }

    pub fn set_incoming_metadata_extension(
        &self,
        attachment: IncomingPeerAttachment,
        supported: bool,
    ) -> Result<(), TorrentPeerError> {
        self.with_state(|state| state.set_incoming_metadata_extension(attachment, supported))?;
        self.publish(true, true)
    }

    pub fn begin_incoming_disconnect(
        &self,
        attachment: IncomingPeerAttachment,
        failure: Option<PeerFailure>,
    ) -> Result<(), TorrentPeerError> {
        let now = self.elapsed();
        self.with_state(|state| state.begin_incoming_disconnect(attachment, failure, now))?;
        self.publish(true, true)
    }

    pub fn remove_incoming(
        &self,
        attachment: IncomingPeerAttachment,
        failure: Option<PeerFailure>,
    ) -> Result<(), TorrentPeerError> {
        let now = self.elapsed();
        self.with_state(|state| state.remove_incoming(attachment, failure, now))?;
        self.unregister_connection_cancellation(attachment.connection_id);
        self.publish(true, true)
    }

    pub fn connection_snapshot(&self) -> Vec<PeerConnectionObservation> {
        self.with_state(|state| state.runtime.snapshot())
    }

    pub fn observe_discovered_peer(
        &self,
        observation: PeerObservation,
    ) -> Result<(), TorrentPeerError> {
        let address = observation.endpoint().address();
        if !self.address_family_policy().permits(address.ip()) {
            return Err(TorrentPeerError::AddressFamilyDenied(address));
        }
        let now = self.elapsed();
        self.with_state(|state| state.registry.observe(observation, now))?;
        self.publish(true, true)
    }

    pub fn remove_discovery_source(&self, source: PeerSource) -> Result<usize, TorrentPeerError> {
        let removed = self.with_state(|state| state.registry.remove_source(source));
        self.publish(true, true)?;
        Ok(removed)
    }

    pub fn enforce_address_families(
        &self,
        policy: AddressFamilyPolicy,
    ) -> Result<usize, TorrentPeerError> {
        self.inner.address_families.replace(policy);
        let connections = self.with_state(|state| {
            state.pex.remove_disallowed(policy, &mut state.registry);
            state.registry.remove_idle_disallowed(policy);
            state
                .runtime
                .snapshot()
                .into_iter()
                .filter(|peer| !policy.permits(peer.endpoint.ip()))
                .map(|peer| peer.connection_id)
                .collect::<Vec<_>>()
        });
        let cancellations = self
            .inner
            .connection_cancellations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for connection in &connections {
            if let Some(cancellation) = cancellations.get(connection) {
                cancellation.cancel();
            }
        }
        drop(cancellations);
        self.publish(true, true)?;
        Ok(connections.len())
    }

    pub fn address_families_converged(&self, policy: AddressFamilyPolicy) -> bool {
        self.with_state(|state| {
            !state.registry.has_disallowed(policy)
                && state
                    .runtime
                    .snapshot()
                    .iter()
                    .all(|peer| policy.permits(peer.endpoint.ip()))
        })
    }

    #[must_use]
    pub fn address_family_policy(&self) -> AddressFamilyPolicy {
        self.inner.address_families.load()
    }

    pub fn registry_snapshot(&self, active: bool) -> PeerRegistrySnapshot {
        let now = self.elapsed();
        self.with_state(|state| registry_snapshot(&state.registry, now, active))
    }

    pub fn publish_active(&self, force: bool) -> Result<(), TorrentPeerError> {
        self.publish(true, force)
    }

    pub fn publish_inactive(&self) -> Result<(), TorrentPeerError> {
        self.publish(false, true)
    }

    pub(crate) fn with_state<T>(&self, operation: impl FnOnce(&mut TorrentPeerState) -> T) -> T {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operation(&mut state)
    }

    pub(crate) fn register_connection_cancellation(
        &self,
        connection: ConnectionId,
        cancellation: CancellationToken,
    ) {
        let replaced = self
            .inner
            .connection_cancellations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(connection, cancellation);
        debug_assert!(
            replaced.is_none(),
            "connection cancellation registered twice"
        );
    }

    pub(crate) fn apply_admission(&self, candidate: ConnectionId, outcome: PeerAdmissionOutcome) {
        let cancellations = self
            .inner
            .connection_cancellations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let loser = match outcome {
            PeerAdmissionOutcome::Admitted { evicted } => evicted,
            PeerAdmissionOutcome::Rejected(_) => Some(candidate),
        };
        if let Some(cancellation) = loser.and_then(|connection| cancellations.get(&connection)) {
            cancellation.cancel();
        }
    }

    pub(crate) fn unregister_connection_cancellation(&self, connection: ConnectionId) {
        self.inner
            .connection_cancellations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&connection);
    }

    pub(crate) fn cancel_incoming_content(&self, attachment: IncomingPeerAttachment) {
        if let Some(cancellation) = self
            .inner
            .connection_cancellations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&attachment.connection_id)
        {
            cancellation.cancel();
        }
    }

    pub(crate) fn set_sink(&self, sink: Arc<dyn TorrentPeerActivitySink>) {
        *self
            .inner
            .sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = sink;
        self.with_state(|state| {
            state.last_connections_emitted.clear();
            state.last_connections_emitted_at = None;
            state.registry_publication = RegistryPublicationState::default();
        });
    }

    pub(crate) fn publish(&self, active: bool, force: bool) -> Result<(), TorrentPeerError> {
        self.publish_at(self.elapsed(), active, force)
    }

    fn publish_at(
        &self,
        captured_at: Duration,
        active: bool,
        force: bool,
    ) -> Result<(), TorrentPeerError> {
        let (connections, registry) = self.with_state(|state| {
            state.refresh_sources()?;
            let current = state.runtime.snapshot();
            let due = force
                || state.last_connections_emitted_at.is_none_or(|previous| {
                    captured_at.saturating_sub(previous) >= PEER_OBSERVATION_INTERVAL
                });
            let connections = if due && state.last_connections_emitted != current {
                state.last_connections_emitted = current.clone();
                state.last_connections_emitted_at = Some(captured_at);
                Some(current)
            } else {
                None
            };

            let registry = registry_publication(state, captured_at, active, force);
            Ok::<_, TorrentPeerError>((connections, registry))
        })?;
        let sink = self
            .inner
            .sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(connections) = connections {
            sink.record_peer_connections(captured_at, connections);
        }
        if let Some(snapshot) = registry {
            sink.record_peer_registry(active, snapshot);
        }
        Ok(())
    }
}

fn registry_snapshot(
    registry: &PeerRegistry,
    captured_at: Duration,
    active: bool,
) -> PeerRegistrySnapshot {
    let mut snapshot = registry.snapshot(PeerSelectionContext { now: captured_at });
    if !active {
        snapshot.records.clear();
        snapshot.counts = PeerRegistryCounts::default();
    }
    snapshot
}

fn registry_publication(
    state: &mut TorrentPeerState,
    captured_at: Duration,
    active: bool,
    force: bool,
) -> Option<PeerRegistrySnapshot> {
    if !force
        && state.registry_publication.active == active
        && state
            .registry_publication
            .next_transition_at
            .is_none_or(|deadline| captured_at < deadline)
    {
        return None;
    }
    let snapshot = registry_snapshot(&state.registry, captured_at, active);
    state.registry_publication.next_transition_at = active
        .then(|| {
            snapshot
                .records
                .iter()
                .filter_map(|record| match record.eligibility {
                    DialEligibility::Backoff { retry_at } if retry_at > captured_at => {
                        Some(retry_at)
                    }
                    _ => None,
                })
                .min()
        })
        .flatten();
    let changed = state.registry_publication.active != active
        || state
            .registry_publication
            .last_emitted
            .as_ref()
            .is_none_or(|previous| {
                previous.maximum_records != snapshot.maximum_records
                    || previous.counts != snapshot.counts
                    || previous.records != snapshot.records
            });
    state.registry_publication.active = active;
    if changed {
        state.registry_publication.last_emitted = Some(snapshot.clone());
        Some(snapshot)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TorrentIncomingStart, TorrentPeerActivitySink, TorrentPeerError, TorrentPeerHandle,
    };
    use crate::AddressFamilyPolicy;
    use crate::peer::{
        PeerEndpoint, PeerObservation, PeerSelectionContext, PeerSelector, PeerSource,
    };
    use crate::peer_runtime::{
        PeerAdmissionOutcome, PeerAdmissionRejection, PeerConnectionDirection,
        PeerConnectionLifecycle, PeerConnectionRole, PeerTransport, PeerUploadActivity,
        PeerUploadGrant,
    };
    use crate::swarm::ConnectionId;
    use rstorrent_protocol::mse::MseMethod;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[derive(Debug, Default)]
    struct RecordingSink {
        connections: Mutex<Vec<Vec<crate::peer_runtime::PeerConnectionObservation>>>,
        registries: Mutex<Vec<(bool, crate::peer::PeerRegistrySnapshot)>>,
    }

    impl TorrentPeerActivitySink for RecordingSink {
        fn record_peer_connections(
            &self,
            _captured_at: Duration,
            peers: Vec<crate::peer_runtime::PeerConnectionObservation>,
        ) {
            self.connections.lock().expect("connections").push(peers);
        }

        fn record_peer_registry(&self, active: bool, snapshot: crate::peer::PeerRegistrySnapshot) {
            self.registries
                .lock()
                .expect("registries")
                .push((active, snapshot));
        }
    }

    #[test]
    fn admission_cancels_only_the_generation_named_as_loser() {
        let (handle, _) = handle();
        let first = ConnectionId::new(1).expect("first");
        let candidate = ConnectionId::new(2).expect("candidate");
        let first_cancellation = CancellationToken::new();
        let candidate_cancellation = CancellationToken::new();
        handle.register_connection_cancellation(first, first_cancellation.clone());
        handle.register_connection_cancellation(candidate, candidate_cancellation.clone());

        handle.apply_admission(
            candidate,
            PeerAdmissionOutcome::Admitted {
                evicted: Some(first),
            },
        );
        assert!(first_cancellation.is_cancelled());
        assert!(!candidate_cancellation.is_cancelled());

        handle.apply_admission(
            candidate,
            PeerAdmissionOutcome::Rejected(PeerAdmissionRejection::DuplicatePeerId {
                winner: first,
            }),
        );
        assert!(candidate_cancellation.is_cancelled());
    }

    fn handle() -> (TorrentPeerHandle, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink::default());
        let handle = TorrentPeerHandle::new(sink.clone()).expect("handle");
        (handle, sink)
    }

    #[test]
    fn ipv4_only_policy_rejects_every_ipv6_peer_source() {
        let (handle, _) = handle();
        handle
            .enforce_address_families(AddressFamilyPolicy::ipv4_only())
            .expect("apply IPv4-only policy");
        let sources = [
            PeerSource::Tracker,
            PeerSource::PeerExchange,
            PeerSource::Dht,
            PeerSource::LocalDiscovery,
            PeerSource::Incoming,
            PeerSource::Manual,
            PeerSource::MagnetHint,
            PeerSource::Cache,
        ];
        for (index, source) in sources.into_iter().enumerate() {
            let endpoint = PeerEndpoint::new(
                format!("[2606:4700:4700::{:x}]:6881", index + 1)
                    .parse()
                    .expect("IPv6 address"),
            )
            .expect("eligible peer endpoint");
            assert!(matches!(
                handle.observe_discovered_peer(PeerObservation::dialable(endpoint, source)),
                Err(TorrentPeerError::AddressFamilyDenied(_))
            ));
        }
        assert!(handle.registry_snapshot(true).records.is_empty());
    }

    #[test]
    fn disabling_ipv6_cancels_plaintext_and_mse_connections_before_convergence() {
        let (handle, _) = handle();
        let plaintext = handle
            .begin_incoming_with_mse(
                "[2606:4700:4700::1111]:51413".parse().expect("remote"),
                "[2606:4700:4700::2222]:6881".parse().expect("local"),
                *b"-LTTEST-000000000000",
                true,
                PeerConnectionRole::Content,
                None,
            )
            .expect("incoming plaintext IPv6 peer");
        handle
            .incoming_handshake_completed(plaintext, *b"-RS0001-LOCALPEER001")
            .expect("complete plaintext IPv6 handshake");
        let encrypted = handle
            .begin_incoming_with_mse(
                "[2606:4700:4700::3333]:51413".parse().expect("remote"),
                "[2606:4700:4700::2222]:6881".parse().expect("local"),
                *b"-LTTEST-000000000001",
                true,
                PeerConnectionRole::Content,
                Some(MseMethod::Rc4),
            )
            .expect("incoming MSE IPv6 peer");
        handle
            .incoming_handshake_completed(encrypted, *b"-RS0001-LOCALPEER002")
            .expect("complete MSE IPv6 handshake");
        let plaintext_cancellation = CancellationToken::new();
        let encrypted_cancellation = CancellationToken::new();
        handle.register_connection_cancellation(
            plaintext.connection_id(),
            plaintext_cancellation.clone(),
        );
        handle.register_connection_cancellation(
            encrypted.connection_id(),
            encrypted_cancellation.clone(),
        );

        assert_eq!(
            handle
                .enforce_address_families(AddressFamilyPolicy::ipv4_only())
                .expect("disable IPv6"),
            2
        );
        assert!(plaintext_cancellation.is_cancelled());
        assert!(encrypted_cancellation.is_cancelled());
        assert!(!handle.address_families_converged(AddressFamilyPolicy::ipv4_only()));
        handle
            .begin_incoming_disconnect(plaintext, None)
            .expect("begin plaintext IPv6 peer retirement");
        handle
            .begin_incoming_disconnect(encrypted, None)
            .expect("begin MSE IPv6 peer retirement");
        handle
            .remove_incoming(plaintext, None)
            .expect("retire plaintext IPv6 peer");
        handle
            .remove_incoming(encrypted, None)
            .expect("retire MSE IPv6 peer");
        handle
            .enforce_address_families(AddressFamilyPolicy::ipv4_only())
            .expect("remove retired IPv6 candidate");
        assert!(handle.address_families_converged(AddressFamilyPolicy::ipv4_only()));
    }

    #[test]
    fn one_allocator_keeps_simultaneous_directions_distinct() {
        let (handle, _) = handle();
        let outgoing = handle.with_state(|state| {
            let endpoint = PeerEndpoint::new("127.0.0.1:6881".parse().expect("endpoint"))
                .expect("valid endpoint");
            state
                .registry
                .observe(
                    PeerObservation::dialable(endpoint, PeerSource::Tracker),
                    Duration::ZERO,
                )
                .expect("observe");
            let candidate = PeerSelector
                .select(
                    &state.registry,
                    PeerSelectionContext {
                        now: Duration::ZERO,
                    },
                )
                .expect("candidate");
            state
                .begin_dial(candidate, PeerConnectionRole::Metadata, Duration::ZERO)
                .expect("dial")
        });
        let incoming = handle
            .begin_incoming(
                "127.0.0.1:51413".parse().expect("remote"),
                "127.0.0.1:43210".parse().expect("local"),
                *b"-LTTEST-000000000000",
                true,
                PeerConnectionRole::Content,
            )
            .expect("incoming");

        assert_eq!(outgoing.connection_id(), ConnectionId::new(1).expect("id"));
        assert_eq!(incoming.connection_id(), ConnectionId::new(2).expect("id"));
        let rows = handle.connection_snapshot();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].direction, PeerConnectionDirection::Outgoing);
        assert_eq!(rows[1].direction, PeerConnectionDirection::Incoming);
        assert_eq!(
            rows[1].lifecycle,
            PeerConnectionLifecycle::ProtocolHandshaking
        );
    }

    #[test]
    fn incoming_record_is_non_connectable_and_survives_until_exact_cleanup() {
        let (handle, sink) = handle();
        let attachment = handle
            .begin_incoming(
                "127.0.0.1:51413".parse().expect("remote"),
                "127.0.0.1:43210".parse().expect("local"),
                *b"-LTTEST-000000000000",
                true,
                PeerConnectionRole::Content,
            )
            .expect("incoming");
        handle
            .incoming_handshake_completed(attachment, *b"-RS0001-LOCALPEER001")
            .expect("connected");
        handle
            .set_incoming_upload(
                attachment,
                PeerUploadActivity {
                    interested: true,
                    grant: PeerUploadGrant::Optimistic,
                    queued_requests: 2,
                    queued_bytes: 32_768,
                    read_active: true,
                    writer_bytes: 16_384,
                    payload_bytes: 65_536,
                    payload_rate: 32_768,
                },
            )
            .expect("activity");
        let registry = handle.registry_snapshot(true);
        assert_eq!(registry.records.len(), 1);
        assert!(!registry.records[0].connectable);
        assert!(registry.records[0].sources.contains(PeerSource::Incoming));
        assert_eq!(
            registry.records[0].eligibility,
            crate::peer::DialEligibility::Connected
        );

        handle
            .begin_incoming_disconnect(attachment, None)
            .expect("disconnecting");
        handle.remove_incoming(attachment, None).expect("remove");
        assert!(handle.connection_snapshot().is_empty());
        assert!(matches!(
            handle.remove_incoming(attachment, None),
            Err(TorrentPeerError::Runtime(_))
        ));
        let emissions = sink.connections.lock().expect("connections");
        assert!(emissions.iter().any(|rows| rows.is_empty()));
    }

    #[test]
    fn incoming_merges_exact_endpoint_sources_and_activity_is_coalesced() {
        let (handle, sink) = handle();
        let remote = "127.0.0.1:51413".parse().expect("remote");
        let endpoint = PeerEndpoint::new(remote).expect("endpoint");
        let attachment = handle.with_state(|state| {
            let tracker = state
                .registry
                .observe(
                    PeerObservation::dialable(endpoint, PeerSource::Tracker),
                    Duration::ZERO,
                )
                .expect("tracker observation");
            let attachment = state
                .begin_incoming(TorrentIncomingStart {
                    remote_endpoint: remote,
                    local_endpoint: "127.0.0.1:43210".parse().expect("local"),
                    peer_id: *b"-LTTEST-000000000000",
                    supports_extensions: true,
                    transport: PeerTransport::Tcp,
                    mse_method: None,
                    role: PeerConnectionRole::Content,
                    now: Duration::ZERO,
                })
                .expect("incoming");
            assert_eq!(attachment.record_id(), tracker.record_id);
            state
                .incoming_handshake_completed(attachment, *b"-RS0001-LOCALPEER001", Duration::ZERO)
                .expect("connected");
            attachment
        });
        handle
            .publish_at(Duration::ZERO, true, true)
            .expect("initial publication");
        let registry = handle.registry_snapshot(true);
        assert_eq!(registry.records.len(), 1);
        assert!(registry.records[0].connectable);
        assert!(registry.records[0].sources.contains(PeerSource::Tracker));
        assert!(registry.records[0].sources.contains(PeerSource::Incoming));

        let activity = |payload_bytes| PeerUploadActivity {
            interested: true,
            grant: PeerUploadGrant::Regular,
            queued_requests: 1,
            queued_bytes: 16_384,
            read_active: false,
            writer_bytes: 0,
            payload_bytes,
            payload_rate: payload_bytes,
        };
        handle
            .with_state(|state| state.set_incoming_upload(attachment, activity(1)))
            .expect("first activity");
        handle
            .publish_at(Duration::from_millis(50), true, false)
            .expect("coalesced publication");
        assert_eq!(sink.connections.lock().expect("connections").len(), 1);

        handle
            .with_state(|state| state.set_incoming_upload(attachment, activity(2)))
            .expect("second activity");
        handle
            .publish_at(Duration::from_millis(100), true, false)
            .expect("due publication");
        let emissions = sink.connections.lock().expect("connections");
        assert_eq!(emissions.len(), 2);
        assert_eq!(
            emissions[1][0]
                .upload
                .expect("upload activity")
                .payload_bytes,
            2
        );
    }

    #[test]
    fn checked_connection_identifier_exhaustion_does_not_wrap() {
        let (handle, _) = handle();
        handle.with_state(|state| state.set_next_connection_id(u64::MAX));
        let result = handle.begin_incoming(
            "127.0.0.1:51413".parse().expect("remote"),
            "127.0.0.1:43210".parse().expect("local"),
            *b"-LTTEST-000000000000",
            false,
            PeerConnectionRole::Content,
        );
        assert!(matches!(
            result,
            Err(TorrentPeerError::ConnectionIdentifierOverflow)
        ));
        assert!(handle.connection_snapshot().is_empty());
        assert!(handle.registry_snapshot(true).records.is_empty());
    }
}

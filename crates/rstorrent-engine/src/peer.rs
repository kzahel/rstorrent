//! Runtime-independent peer discovery, registry, and dial state.

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use crate::network::{AddressFamilyPolicy, is_valid_outbound_address};
use crate::swarm::ConnectionId;

pub const DEFAULT_MAX_PEER_RECORDS: usize = 1_000;
pub const DEFAULT_MAX_CONSECUTIVE_FAILURES: u32 = 3;
pub const DEFAULT_RECONNECT_BACKOFF: Duration = Duration::from_secs(60);
pub const UTP_RETRY_INITIAL_BACKOFF: Duration = Duration::from_secs(5 * 60);
pub const UTP_RETRY_MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PeerEndpoint(SocketAddr);

impl PeerEndpoint {
    pub fn new(address: SocketAddr) -> Result<Self, PeerRegistryError> {
        if !is_valid_outbound_address(address) {
            return Err(PeerRegistryError::InvalidEndpoint(address));
        }
        Ok(Self(address))
    }

    pub fn address(self) -> SocketAddr {
        self.0
    }
}

impl fmt::Display for PeerEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PeerSource {
    Tracker,
    PeerExchange,
    Dht,
    LocalDiscovery,
    Incoming,
    Manual,
    MagnetHint,
    Cache,
}

impl PeerSource {
    const fn mask(self) -> u16 {
        1_u16 << self as u8
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerSources(u16);

impl PeerSources {
    pub fn from_source(source: PeerSource) -> Self {
        Self(source.mask())
    }

    pub fn insert(&mut self, source: PeerSource) {
        self.0 |= source.mask();
    }

    pub fn remove(&mut self, source: PeerSource) {
        self.0 &= !source.mask();
    }

    pub fn contains(self, source: PeerSource) -> bool {
        self.0 & source.mask() != 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerObservation {
    endpoint: PeerEndpoint,
    source: PeerSource,
    connectable: bool,
    utp_advertised: bool,
}

impl PeerObservation {
    pub fn new(endpoint: PeerEndpoint, source: PeerSource, connectable: bool) -> Self {
        Self {
            endpoint,
            source,
            connectable,
            utp_advertised: false,
        }
    }

    pub fn dialable(endpoint: PeerEndpoint, source: PeerSource) -> Self {
        Self::new(endpoint, source, true)
    }

    pub fn pex_dialable(endpoint: PeerEndpoint, utp_advertised: bool) -> Self {
        Self {
            endpoint,
            source: PeerSource::PeerExchange,
            connectable: true,
            utp_advertised,
        }
    }

    pub fn endpoint(self) -> PeerEndpoint {
        self.endpoint
    }

    pub fn source(self) -> PeerSource {
        self.source
    }

    pub fn is_connectable(self) -> bool {
        self.connectable
    }

    pub fn utp_advertised(self) -> bool {
        self.utp_advertised
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PeerRecordId(u64);

impl PeerRecordId {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PeerRecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DialAttemptId(u64);

impl DialAttemptId {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DialAttemptId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerPhase {
    Idle,
    Dialing { attempt_id: DialAttemptId },
    Connected { attempt_id: DialAttemptId },
    Banned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerFailure {
    Connect,
    Handshake,
    SelfConnection,
    DuplicatePeerId,
    Protocol,
    RemoteClosed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MseEndpointState {
    #[default]
    Unknown,
    MseCapable,
    PlainPreferred,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UtpEndpointState {
    #[default]
    Unknown,
    Advertised,
    Confirmed,
    Suppressed {
        failures: u8,
        retry_at: Duration,
    },
}

impl UtpEndpointState {
    pub fn should_try(self, now: Duration) -> bool {
        match self {
            Self::Unknown | Self::Advertised | Self::Confirmed => true,
            Self::Suppressed { retry_at, .. } => now >= retry_at,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UtpDialDecision {
    Try,
    TcpWhileSuppressed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UtpConnectOutcome {
    Connected,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerIntegrity {
    pub trust_points: i8,
    pub hash_failures: u8,
    pub valid_pieces: u32,
    pub on_parole: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerIntegrityAction {
    Retain,
    Ban,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerHistory {
    pub dial_attempts: u32,
    pub total_failures: u32,
    pub consecutive_failures: u32,
    pub last_dial_at: Option<Duration>,
    pub last_connected_at: Option<Duration>,
    pub last_disconnected_at: Option<Duration>,
    pub retry_at: Option<Duration>,
    pub last_failure: Option<PeerFailure>,
    pub mse_endpoint: MseEndpointState,
    pub utp_endpoint: UtpEndpointState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerTransferTotals {
    pub payload_downloaded_bytes: u64,
    pub payload_uploaded_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerRecord {
    id: PeerRecordId,
    endpoint: PeerEndpoint,
    sources: PeerSources,
    connectable: bool,
    first_observed_at: Duration,
    last_observed_at: Duration,
    observation_order: u64,
    phase: PeerPhase,
    history: PeerHistory,
    transfers: PeerTransferTotals,
    integrity: PeerIntegrity,
    last_connection_attempt: Option<DialAttemptId>,
    incoming_connections: u32,
    ban_when_idle: bool,
    last_utp_outcome_attempt: Option<DialAttemptId>,
}

impl PeerRecord {
    pub fn id(&self) -> PeerRecordId {
        self.id
    }

    pub fn endpoint(&self) -> PeerEndpoint {
        self.endpoint
    }

    pub fn sources(&self) -> PeerSources {
        self.sources
    }

    pub fn is_connectable(&self) -> bool {
        self.connectable
    }

    pub fn first_observed_at(&self) -> Duration {
        self.first_observed_at
    }

    pub fn last_observed_at(&self) -> Duration {
        self.last_observed_at
    }

    pub fn phase(&self) -> PeerPhase {
        self.phase
    }

    pub fn history(&self) -> PeerHistory {
        self.history
    }

    pub fn transfers(&self) -> PeerTransferTotals {
        self.transfers
    }

    pub fn integrity(&self) -> PeerIntegrity {
        self.integrity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerRegistryConfig {
    pub max_records: usize,
    pub max_consecutive_failures: u32,
    pub reconnect_backoff: Duration,
}

impl Default for PeerRegistryConfig {
    fn default() -> Self {
        Self {
            max_records: DEFAULT_MAX_PEER_RECORDS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            reconnect_backoff: DEFAULT_RECONNECT_BACKOFF,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerObservationDisposition {
    Added,
    Merged,
    Replaced { evicted: PeerRecordId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerObservationResult {
    pub record_id: PeerRecordId,
    pub disposition: PeerObservationDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerSelectionContext {
    pub now: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialEligibility {
    Eligible,
    NotConnectable,
    Dialing,
    Connected,
    Banned,
    Backoff { retry_at: Duration },
    FailureLimit { failures: u32, maximum: u32 },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerRegistryCounts {
    pub total: usize,
    pub eligible: usize,
    pub not_connectable: usize,
    pub dialing: usize,
    pub connected: usize,
    pub banned: usize,
    pub backed_off: usize,
    pub failure_limited: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerRecordSnapshot {
    pub id: PeerRecordId,
    pub endpoint: PeerEndpoint,
    pub sources: PeerSources,
    pub connectable: bool,
    pub first_observed_at: Duration,
    pub last_observed_at: Duration,
    pub phase: PeerPhase,
    pub eligibility: DialEligibility,
    pub history: PeerHistory,
    pub transfers: PeerTransferTotals,
    pub integrity: PeerIntegrity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerRegistrySnapshot {
    pub captured_at: Duration,
    pub maximum_records: usize,
    pub counts: PeerRegistryCounts,
    pub records: Vec<PeerRecordSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialCandidate {
    record_id: PeerRecordId,
    endpoint: PeerEndpoint,
}

impl DialCandidate {
    pub fn record_id(self) -> PeerRecordId {
        self.record_id
    }

    pub fn endpoint(self) -> PeerEndpoint {
        self.endpoint
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialAttempt {
    id: DialAttemptId,
    connection_id: ConnectionId,
    record_id: PeerRecordId,
    endpoint: PeerEndpoint,
    mse_endpoint: MseEndpointState,
    utp_decision: UtpDialDecision,
}

impl DialAttempt {
    pub fn id(self) -> DialAttemptId {
        self.id
    }

    pub fn connection_id(self) -> ConnectionId {
        self.connection_id
    }

    pub fn record_id(self) -> PeerRecordId {
        self.record_id
    }

    pub fn endpoint(self) -> PeerEndpoint {
        self.endpoint
    }

    pub fn mse_endpoint(self) -> MseEndpointState {
        self.mse_endpoint
    }

    pub fn utp_decision(self) -> UtpDialDecision {
        self.utp_decision
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PeerSelector;

impl PeerSelector {
    pub fn eligibility(
        self,
        record: &PeerRecord,
        context: PeerSelectionContext,
        config: PeerRegistryConfig,
    ) -> DialEligibility {
        if record.incoming_connections != 0 {
            return DialEligibility::Connected;
        }
        match record.phase {
            PeerPhase::Dialing { .. } => return DialEligibility::Dialing,
            PeerPhase::Connected { .. } => return DialEligibility::Connected,
            PeerPhase::Banned => return DialEligibility::Banned,
            PeerPhase::Idle => {}
        }
        if !record.connectable {
            return DialEligibility::NotConnectable;
        }
        if record.history.consecutive_failures >= config.max_consecutive_failures {
            return DialEligibility::FailureLimit {
                failures: record.history.consecutive_failures,
                maximum: config.max_consecutive_failures,
            };
        }
        if let Some(retry_at) = record.history.retry_at
            && context.now < retry_at
        {
            return DialEligibility::Backoff { retry_at };
        }
        DialEligibility::Eligible
    }

    pub fn select(
        self,
        registry: &PeerRegistry,
        context: PeerSelectionContext,
    ) -> Option<DialCandidate> {
        self.select_with_address_families(registry, context, AddressFamilyPolicy::dual_stack())
    }

    pub fn select_with_address_families(
        self,
        registry: &PeerRegistry,
        context: PeerSelectionContext,
        address_families: AddressFamilyPolicy,
    ) -> Option<DialCandidate> {
        registry
            .records
            .iter()
            .filter(|record| {
                address_families.permits(record.endpoint.address().ip())
                    && self.eligibility(record, context, registry.config)
                        == DialEligibility::Eligible
            })
            .min_by(|left, right| compare_dial_candidates(left, right))
            .map(|record| DialCandidate {
                record_id: record.id,
                endpoint: record.endpoint,
            })
    }
}

fn compare_dial_candidates(left: &PeerRecord, right: &PeerRecord) -> Ordering {
    left.history
        .consecutive_failures
        .cmp(&right.history.consecutive_failures)
        .then_with(
            || match (left.history.last_dial_at, right.history.last_dial_at) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(left), Some(right)) => left.cmp(&right),
            },
        )
        .then_with(|| source_rank(right.sources).cmp(&source_rank(left.sources)))
        .then_with(|| left.observation_order.cmp(&right.observation_order))
        .then_with(|| left.id.cmp(&right.id))
}

fn source_rank(sources: PeerSources) -> u8 {
    [
        (PeerSource::Manual, 8),
        (PeerSource::MagnetHint, 7),
        (PeerSource::LocalDiscovery, 6),
        (PeerSource::Tracker, 5),
        (PeerSource::Incoming, 4),
        (PeerSource::PeerExchange, 3),
        (PeerSource::Dht, 2),
        (PeerSource::Cache, 1),
    ]
    .into_iter()
    .filter_map(|(source, rank)| sources.contains(source).then_some(rank))
    .max()
    .unwrap_or(0)
}

#[derive(Debug)]
pub struct PeerRegistry {
    config: PeerRegistryConfig,
    records: Vec<PeerRecord>,
    next_record_id: u64,
    next_attempt_id: u64,
    next_observation_order: u64,
}

impl PeerRegistry {
    pub fn new(config: PeerRegistryConfig) -> Result<Self, PeerRegistryError> {
        if config.max_records == 0 {
            return Err(PeerRegistryError::ZeroCapacity);
        }
        if config.max_consecutive_failures == 0 {
            return Err(PeerRegistryError::ZeroFailureLimit);
        }
        Ok(Self {
            config,
            records: Vec::new(),
            next_record_id: 1,
            next_attempt_id: 1,
            next_observation_order: 1,
        })
    }

    pub fn config(&self) -> PeerRegistryConfig {
        self.config
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn records(&self) -> impl ExactSizeIterator<Item = &PeerRecord> {
        self.records.iter()
    }

    pub fn get(&self, id: PeerRecordId) -> Option<&PeerRecord> {
        self.records.iter().find(|record| record.id == id)
    }

    pub fn find_endpoint(&self, endpoint: PeerEndpoint) -> Option<&PeerRecord> {
        self.records
            .iter()
            .find(|record| record.endpoint == endpoint)
    }

    pub fn snapshot(&self, context: PeerSelectionContext) -> PeerRegistrySnapshot {
        let selector = PeerSelector;
        let records = self
            .records
            .iter()
            .map(|record| {
                let eligibility = selector.eligibility(record, context, self.config);
                PeerRecordSnapshot {
                    id: record.id,
                    endpoint: record.endpoint,
                    sources: record.sources,
                    connectable: record.connectable,
                    first_observed_at: record.first_observed_at,
                    last_observed_at: record.last_observed_at,
                    phase: record.phase,
                    eligibility,
                    history: record.history,
                    transfers: record.transfers,
                    integrity: record.integrity,
                }
            })
            .collect();
        PeerRegistrySnapshot {
            captured_at: context.now,
            maximum_records: self.config.max_records,
            counts: self.counts(context),
            records,
        }
    }

    pub fn counts(&self, context: PeerSelectionContext) -> PeerRegistryCounts {
        let selector = PeerSelector;
        let mut counts = PeerRegistryCounts {
            total: self.records.len(),
            ..PeerRegistryCounts::default()
        };
        for record in &self.records {
            match selector.eligibility(record, context, self.config) {
                DialEligibility::Eligible => counts.eligible += 1,
                DialEligibility::NotConnectable => counts.not_connectable += 1,
                DialEligibility::Dialing => counts.dialing += 1,
                DialEligibility::Connected => counts.connected += 1,
                DialEligibility::Banned => counts.banned += 1,
                DialEligibility::Backoff { .. } => counts.backed_off += 1,
                DialEligibility::FailureLimit { .. } => counts.failure_limited += 1,
            }
        }
        counts
    }

    /// Remove one discovery source and discard now-source-less idle records.
    pub fn remove_source(&mut self, source: PeerSource) -> usize {
        let before = self.records.len();
        for record in &mut self.records {
            record.sources.remove(source);
        }
        self.records
            .retain(|record| !record.sources.is_empty() || record.phase != PeerPhase::Idle);
        before - self.records.len()
    }

    pub(crate) fn remove_idle_disallowed(&mut self, policy: AddressFamilyPolicy) -> usize {
        let before = self.records.len();
        self.records.retain(|record| {
            policy.permits(record.endpoint.address().ip())
                || record.phase != PeerPhase::Idle
                || record.incoming_connections != 0
        });
        before - self.records.len()
    }

    pub(crate) fn has_disallowed(&self, policy: AddressFamilyPolicy) -> bool {
        self.records
            .iter()
            .any(|record| !policy.permits(record.endpoint.address().ip()))
    }

    /// Remove one discovery source from one endpoint and discard a now-
    /// source-less idle record.
    pub fn remove_endpoint_source(&mut self, endpoint: PeerEndpoint, source: PeerSource) -> bool {
        let Some(index) = self
            .records
            .iter()
            .position(|record| record.endpoint == endpoint)
        else {
            return false;
        };
        self.records[index].sources.remove(source);
        if self.records[index].sources.is_empty() && self.records[index].phase == PeerPhase::Idle {
            self.records.swap_remove(index);
            true
        } else {
            false
        }
    }

    pub fn observe(
        &mut self,
        observation: PeerObservation,
        now: Duration,
    ) -> Result<PeerObservationResult, PeerRegistryError> {
        if let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.endpoint == observation.endpoint)
        {
            record.sources.insert(observation.source);
            record.connectable |= observation.connectable;
            record.last_observed_at = now;
            if observation.utp_advertised
                && record.history.utp_endpoint != UtpEndpointState::Confirmed
            {
                record.history.utp_endpoint = UtpEndpointState::Advertised;
            }
            return Ok(PeerObservationResult {
                record_id: record.id,
                disposition: PeerObservationDisposition::Merged,
            });
        }

        let id = PeerRecordId(self.next_record_id);
        let next_record_id = self
            .next_record_id
            .checked_add(1)
            .ok_or(PeerRegistryError::IdentifierOverflow("peer record"))?;
        let observation_order = self.next_observation_order;
        let next_observation_order = self.next_observation_order.checked_add(1).ok_or(
            PeerRegistryError::IdentifierOverflow("peer observation order"),
        )?;

        let disposition = if self.records.len() == self.config.max_records {
            let evict_index =
                self.eviction_candidate()
                    .ok_or(PeerRegistryError::CapacityExhausted {
                        maximum: self.config.max_records,
                    })?;
            let evicted = self.records.swap_remove(evict_index).id;
            PeerObservationDisposition::Replaced { evicted }
        } else {
            PeerObservationDisposition::Added
        };

        let mut history = PeerHistory::default();
        if observation.utp_advertised {
            history.utp_endpoint = UtpEndpointState::Advertised;
        }
        self.records.push(PeerRecord {
            id,
            endpoint: observation.endpoint,
            sources: PeerSources::from_source(observation.source),
            connectable: observation.connectable,
            first_observed_at: now,
            last_observed_at: now,
            observation_order,
            phase: PeerPhase::Idle,
            history,
            transfers: PeerTransferTotals::default(),
            integrity: PeerIntegrity::default(),
            last_connection_attempt: None,
            incoming_connections: 0,
            ban_when_idle: false,
            last_utp_outcome_attempt: None,
        });
        self.next_record_id = next_record_id;
        self.next_observation_order = next_observation_order;

        Ok(PeerObservationResult {
            record_id: id,
            disposition,
        })
    }

    pub fn begin_dial(
        &mut self,
        candidate: DialCandidate,
        context: PeerSelectionContext,
    ) -> Result<DialAttempt, PeerRegistryError> {
        let connection_id = ConnectionId::new(self.next_attempt_id)
            .ok_or(PeerRegistryError::IdentifierOverflow("connection"))?;
        self.begin_dial_with_connection_id(candidate, context, connection_id)
    }

    pub(crate) fn begin_dial_with_connection_id(
        &mut self,
        candidate: DialCandidate,
        context: PeerSelectionContext,
        connection_id: ConnectionId,
    ) -> Result<DialAttempt, PeerRegistryError> {
        let record_index = self
            .record_index(candidate.record_id)
            .ok_or(PeerRegistryError::UnknownRecord(candidate.record_id))?;
        if self.records[record_index].endpoint != candidate.endpoint {
            return Err(PeerRegistryError::CandidateChanged(candidate.record_id));
        }
        let eligibility =
            PeerSelector.eligibility(&self.records[record_index], context, self.config);
        if eligibility != DialEligibility::Eligible {
            return Err(PeerRegistryError::Ineligible {
                record_id: candidate.record_id,
                eligibility,
            });
        }

        let attempt_id = DialAttemptId(self.next_attempt_id);
        let next_attempt_id = self
            .next_attempt_id
            .checked_add(1)
            .ok_or(PeerRegistryError::IdentifierOverflow("dial attempt"))?;
        let dial_attempts = self.records[record_index]
            .history
            .dial_attempts
            .checked_add(1)
            .ok_or(PeerRegistryError::HistoryOverflow(candidate.record_id))?;

        let record = &mut self.records[record_index];
        record.phase = PeerPhase::Dialing { attempt_id };
        record.history.dial_attempts = dial_attempts;
        record.history.last_dial_at = Some(context.now);
        self.next_attempt_id = next_attempt_id;

        Ok(DialAttempt {
            id: attempt_id,
            connection_id,
            record_id: candidate.record_id,
            endpoint: candidate.endpoint,
            mse_endpoint: record.history.mse_endpoint,
            utp_decision: if record.history.utp_endpoint.should_try(context.now) {
                UtpDialDecision::Try
            } else {
                UtpDialDecision::TcpWhileSuppressed
            },
        })
    }

    pub fn record_utp_outcome(
        &mut self,
        attempt: DialAttempt,
        now: Duration,
        outcome: UtpConnectOutcome,
    ) -> Result<(), PeerRegistryError> {
        if attempt.utp_decision != UtpDialDecision::Try {
            return Err(PeerRegistryError::UnexpectedUtpOutcome(attempt.id));
        }
        let record = self.record_for_attempt_mut(attempt, false)?;
        if record.last_utp_outcome_attempt == Some(attempt.id) {
            return Err(PeerRegistryError::DuplicateUtpOutcome(attempt.id));
        }
        record.history.utp_endpoint = match outcome {
            UtpConnectOutcome::Connected => UtpEndpointState::Confirmed,
            UtpConnectOutcome::Failed => {
                let failures = match record.history.utp_endpoint {
                    UtpEndpointState::Suppressed { failures, .. } => failures.saturating_add(1),
                    UtpEndpointState::Unknown
                    | UtpEndpointState::Advertised
                    | UtpEndpointState::Confirmed => 1,
                };
                let retry_at = now
                    .checked_add(utp_retry_backoff(failures))
                    .ok_or(PeerRegistryError::TimeOverflow(record.id))?;
                UtpEndpointState::Suppressed { failures, retry_at }
            }
        };
        record.last_utp_outcome_attempt = Some(attempt.id);
        Ok(())
    }

    pub fn update_mse_endpoint(
        &mut self,
        attempt: DialAttempt,
        state: MseEndpointState,
    ) -> Result<(), PeerRegistryError> {
        let record = self.record_for_attempt_mut(attempt, false)?;
        record.history.mse_endpoint = state;
        Ok(())
    }

    pub(crate) fn incoming_connected(
        &mut self,
        record_id: PeerRecordId,
        now: Duration,
    ) -> Result<(), PeerRegistryError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .ok_or(PeerRegistryError::UnknownRecord(record_id))?;
        record.incoming_connections = record
            .incoming_connections
            .checked_add(1)
            .ok_or(PeerRegistryError::HistoryOverflow(record_id))?;
        record.history.last_connected_at = Some(now);
        Ok(())
    }

    pub(crate) fn incoming_closed(
        &mut self,
        record_id: PeerRecordId,
        now: Duration,
        failure: Option<PeerFailure>,
    ) -> Result<(), PeerRegistryError> {
        let backoff = self.config.reconnect_backoff;
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .ok_or(PeerRegistryError::UnknownRecord(record_id))?;
        let Some(incoming_connections) = record.incoming_connections.checked_sub(1) else {
            return Err(PeerRegistryError::InactiveIncoming(record_id));
        };
        record.incoming_connections = incoming_connections;
        record.history.last_disconnected_at = Some(now);
        if let Some(failure) = failure {
            apply_failure(record, now, failure, backoff)?;
        }
        settle_pending_ban(record);
        Ok(())
    }

    pub(crate) fn record_transfer(
        &mut self,
        record_id: PeerRecordId,
        transfer: PeerTransferTotals,
    ) -> Result<(), PeerRegistryError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .ok_or(PeerRegistryError::UnknownRecord(record_id))?;
        record.transfers.payload_downloaded_bytes = record
            .transfers
            .payload_downloaded_bytes
            .saturating_add(transfer.payload_downloaded_bytes);
        record.transfers.payload_uploaded_bytes = record
            .transfers
            .payload_uploaded_bytes
            .saturating_add(transfer.payload_uploaded_bytes);
        Ok(())
    }

    pub fn dial_failed(
        &mut self,
        attempt: DialAttempt,
        now: Duration,
        failure: PeerFailure,
    ) -> Result<(), PeerRegistryError> {
        let backoff = self.config.reconnect_backoff;
        let record = self.record_for_attempt_mut(attempt, false)?;
        record.phase = PeerPhase::Idle;
        apply_failure(record, now, failure, backoff)?;
        settle_pending_ban(record);
        Ok(())
    }

    pub fn dial_cancelled(&mut self, attempt: DialAttempt) -> Result<(), PeerRegistryError> {
        let record = self.record_for_attempt_mut(attempt, false)?;
        record.phase = PeerPhase::Idle;
        settle_pending_ban(record);
        Ok(())
    }

    pub fn dial_succeeded(
        &mut self,
        attempt: DialAttempt,
        now: Duration,
    ) -> Result<(), PeerRegistryError> {
        let record = self.record_for_attempt_mut(attempt, false)?;
        record.phase = PeerPhase::Connected {
            attempt_id: attempt.id,
        };
        record.history.consecutive_failures = 0;
        record.history.retry_at = None;
        record.history.last_failure = None;
        record.history.last_connected_at = Some(now);
        record.last_connection_attempt = Some(attempt.id);
        Ok(())
    }

    pub fn connection_closed(
        &mut self,
        attempt: DialAttempt,
        now: Duration,
        failure: Option<PeerFailure>,
    ) -> Result<(), PeerRegistryError> {
        let backoff = self.config.reconnect_backoff;
        let record = self.record_for_attempt_mut(attempt, true)?;
        record.phase = PeerPhase::Idle;
        record.history.last_disconnected_at = Some(now);
        if let Some(failure) = failure {
            apply_failure(record, now, failure, backoff)?;
        }
        settle_pending_ban(record);
        Ok(())
    }

    pub fn ban(&mut self, id: PeerRecordId) -> Result<(), PeerRegistryError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(PeerRegistryError::UnknownRecord(id))?;
        if record.incoming_connections != 0 {
            return Err(PeerRegistryError::ActivePeer(id));
        }
        match record.phase {
            PeerPhase::Idle | PeerPhase::Banned => {
                record.phase = PeerPhase::Banned;
                record.ban_when_idle = false;
                Ok(())
            }
            PeerPhase::Dialing { .. } | PeerPhase::Connected { .. } => {
                Err(PeerRegistryError::ActivePeer(id))
            }
        }
    }

    pub fn record_piece_passed(&mut self, attempt: DialAttempt) -> Result<(), PeerRegistryError> {
        let record = self.record_for_integrity_mut(attempt)?;
        record.integrity.trust_points = record.integrity.trust_points.saturating_add(1).min(8);
        record.integrity.valid_pieces = record.integrity.valid_pieces.saturating_add(1);
        record.integrity.on_parole = false;
        Ok(())
    }

    pub fn record_piece_failed(
        &mut self,
        attempt: DialAttempt,
        known_bad: bool,
    ) -> Result<PeerIntegrityAction, PeerRegistryError> {
        let record = self.record_for_integrity_mut(attempt)?;
        record.integrity.trust_points = record.integrity.trust_points.saturating_sub(2).max(-7);
        record.integrity.hash_failures = record.integrity.hash_failures.saturating_add(1);
        record.integrity.on_parole = true;
        if known_bad || record.integrity.trust_points <= -7 {
            record.ban_when_idle = true;
            Ok(PeerIntegrityAction::Ban)
        } else {
            Ok(PeerIntegrityAction::Retain)
        }
    }

    pub(crate) fn record_incoming_piece_passed(
        &mut self,
        record_id: PeerRecordId,
    ) -> Result<(), PeerRegistryError> {
        let record = self.record_for_incoming_integrity_mut(record_id)?;
        record.integrity.trust_points = record.integrity.trust_points.saturating_add(1).min(8);
        record.integrity.valid_pieces = record.integrity.valid_pieces.saturating_add(1);
        record.integrity.on_parole = false;
        Ok(())
    }

    pub(crate) fn record_incoming_piece_failed(
        &mut self,
        record_id: PeerRecordId,
        known_bad: bool,
    ) -> Result<PeerIntegrityAction, PeerRegistryError> {
        let record = self.record_for_incoming_integrity_mut(record_id)?;
        record.integrity.trust_points = record.integrity.trust_points.saturating_sub(2).max(-7);
        record.integrity.hash_failures = record.integrity.hash_failures.saturating_add(1);
        record.integrity.on_parole = true;
        if known_bad || record.integrity.trust_points <= -7 {
            record.ban_when_idle = true;
            Ok(PeerIntegrityAction::Ban)
        } else {
            Ok(PeerIntegrityAction::Retain)
        }
    }

    fn record_for_incoming_integrity_mut(
        &mut self,
        record_id: PeerRecordId,
    ) -> Result<&mut PeerRecord, PeerRegistryError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .ok_or(PeerRegistryError::UnknownRecord(record_id))?;
        if record.incoming_connections == 0 {
            return Err(PeerRegistryError::UnknownRecord(record_id));
        }
        Ok(record)
    }

    fn record_index(&self, id: PeerRecordId) -> Option<usize> {
        self.records.iter().position(|record| record.id == id)
    }

    fn record_for_attempt_mut(
        &mut self,
        attempt: DialAttempt,
        connected: bool,
    ) -> Result<&mut PeerRecord, PeerRegistryError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == attempt.record_id)
            .ok_or(PeerRegistryError::UnknownRecord(attempt.record_id))?;
        if record.endpoint != attempt.endpoint {
            return Err(PeerRegistryError::StaleAttempt(attempt.id));
        }
        let expected = if connected {
            PeerPhase::Connected {
                attempt_id: attempt.id,
            }
        } else {
            PeerPhase::Dialing {
                attempt_id: attempt.id,
            }
        };
        if record.phase != expected {
            return Err(PeerRegistryError::StaleAttempt(attempt.id));
        }
        Ok(record)
    }

    fn record_for_integrity_mut(
        &mut self,
        attempt: DialAttempt,
    ) -> Result<&mut PeerRecord, PeerRegistryError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == attempt.record_id)
            .ok_or(PeerRegistryError::UnknownRecord(attempt.record_id))?;
        let current_generation = match record.phase {
            PeerPhase::Connected { attempt_id } => attempt_id == attempt.id,
            PeerPhase::Idle | PeerPhase::Banned => {
                record.last_connection_attempt == Some(attempt.id)
            }
            PeerPhase::Dialing { .. } => false,
        };
        if record.endpoint != attempt.endpoint || !current_generation {
            return Err(PeerRegistryError::StaleAttempt(attempt.id));
        }
        Ok(record)
    }

    fn eviction_candidate(&self) -> Option<usize> {
        self.records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.phase == PeerPhase::Idle && record.incoming_connections == 0
            })
            .max_by(|(_, left), (_, right)| compare_eviction_candidates(left, right, self.config))
            .map(|(index, _)| index)
    }
}

fn settle_pending_ban(record: &mut PeerRecord) {
    if record.ban_when_idle && record.incoming_connections == 0 && record.phase == PeerPhase::Idle {
        record.phase = PeerPhase::Banned;
        record.ban_when_idle = false;
    }
}

fn apply_failure(
    record: &mut PeerRecord,
    now: Duration,
    failure: PeerFailure,
    reconnect_backoff: Duration,
) -> Result<(), PeerRegistryError> {
    let total_failures = record
        .history
        .total_failures
        .checked_add(1)
        .ok_or(PeerRegistryError::HistoryOverflow(record.id))?;
    let consecutive_failures = record
        .history
        .consecutive_failures
        .checked_add(1)
        .ok_or(PeerRegistryError::HistoryOverflow(record.id))?;
    let backoff = reconnect_backoff
        .checked_mul(consecutive_failures)
        .ok_or(PeerRegistryError::TimeOverflow(record.id))?;
    let retry_at = now
        .checked_add(backoff)
        .ok_or(PeerRegistryError::TimeOverflow(record.id))?;

    record.history.total_failures = total_failures;
    record.history.consecutive_failures = consecutive_failures;
    record.history.retry_at = Some(retry_at);
    record.history.last_failure = Some(failure);
    Ok(())
}

fn utp_retry_backoff(failures: u8) -> Duration {
    let shift = u32::from(failures.saturating_sub(1)).min(4);
    UTP_RETRY_INITIAL_BACKOFF
        .checked_mul(1_u32 << shift)
        .unwrap_or(UTP_RETRY_MAX_BACKOFF)
        .min(UTP_RETRY_MAX_BACKOFF)
}

fn compare_eviction_candidates(
    left: &PeerRecord,
    right: &PeerRecord,
    config: PeerRegistryConfig,
) -> Ordering {
    let left_at_limit = left.history.consecutive_failures >= config.max_consecutive_failures;
    let right_at_limit = right.history.consecutive_failures >= config.max_consecutive_failures;
    left_at_limit
        .cmp(&right_at_limit)
        .then_with(|| (!left.connectable).cmp(&!right.connectable))
        .then_with(|| {
            left.history
                .consecutive_failures
                .cmp(&right.history.consecutive_failures)
        })
        .then_with(|| right.last_observed_at.cmp(&left.last_observed_at))
        .then_with(|| right.observation_order.cmp(&left.observation_order))
}

#[derive(Debug)]
pub enum PeerRegistryError {
    InvalidEndpoint(SocketAddr),
    ZeroCapacity,
    ZeroFailureLimit,
    CapacityExhausted {
        maximum: usize,
    },
    UnknownRecord(PeerRecordId),
    CandidateChanged(PeerRecordId),
    Ineligible {
        record_id: PeerRecordId,
        eligibility: DialEligibility,
    },
    StaleAttempt(DialAttemptId),
    DuplicateUtpOutcome(DialAttemptId),
    UnexpectedUtpOutcome(DialAttemptId),
    ActivePeer(PeerRecordId),
    InactiveIncoming(PeerRecordId),
    IdentifierOverflow(&'static str),
    HistoryOverflow(PeerRecordId),
    TimeOverflow(PeerRecordId),
}

impl fmt::Display for PeerRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint(endpoint) => {
                write!(formatter, "invalid peer endpoint {endpoint}")
            }
            Self::ZeroCapacity => write!(formatter, "peer registry capacity must be nonzero"),
            Self::ZeroFailureLimit => {
                write!(formatter, "peer failure limit must be nonzero")
            }
            Self::CapacityExhausted { maximum } => {
                write!(
                    formatter,
                    "peer registry capacity {maximum} is occupied by active or banned records"
                )
            }
            Self::UnknownRecord(record_id) => write!(formatter, "unknown peer record {record_id}"),
            Self::CandidateChanged(record_id) => {
                write!(
                    formatter,
                    "peer candidate {record_id} changed before dialing"
                )
            }
            Self::Ineligible {
                record_id,
                eligibility,
            } => write!(
                formatter,
                "peer record {record_id} is not dial eligible: {eligibility:?}"
            ),
            Self::StaleAttempt(attempt_id) => {
                write!(formatter, "dial attempt {attempt_id} is stale")
            }
            Self::DuplicateUtpOutcome(attempt_id) => {
                write!(
                    formatter,
                    "dial attempt {attempt_id} already recorded a uTP outcome"
                )
            }
            Self::UnexpectedUtpOutcome(attempt_id) => {
                write!(formatter, "dial attempt {attempt_id} did not select uTP")
            }
            Self::ActivePeer(record_id) => {
                write!(formatter, "peer record {record_id} is active")
            }
            Self::InactiveIncoming(record_id) => {
                write!(
                    formatter,
                    "peer record {record_id} has no active incoming connection"
                )
            }
            Self::IdentifierOverflow(kind) => write!(formatter, "{kind} identifier overflow"),
            Self::HistoryOverflow(record_id) => {
                write!(
                    formatter,
                    "peer record {record_id} history counter overflow"
                )
            }
            Self::TimeOverflow(record_id) => {
                write!(formatter, "peer record {record_id} reconnect time overflow")
            }
        }
    }
}

impl Error for PeerRegistryError {}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};
    use std::time::Duration;

    use super::{
        DialEligibility, PeerEndpoint, PeerFailure, PeerIntegrityAction, PeerObservation,
        PeerObservationDisposition, PeerPhase, PeerRegistry, PeerRegistryConfig, PeerRegistryError,
        PeerSelectionContext, PeerSelector, PeerSource, PeerTransferTotals,
        UTP_RETRY_INITIAL_BACKOFF, UTP_RETRY_MAX_BACKOFF, UtpConnectOutcome, UtpDialDecision,
        UtpEndpointState,
    };

    fn endpoint(port: u16) -> PeerEndpoint {
        PeerEndpoint::new(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).expect("valid endpoint")
    }

    fn config(max_records: usize) -> PeerRegistryConfig {
        PeerRegistryConfig {
            max_records,
            max_consecutive_failures: 3,
            reconnect_backoff: Duration::from_secs(10),
        }
    }

    #[test]
    fn observations_merge_sources_and_strengthen_reachability() {
        let mut registry = PeerRegistry::new(config(4)).expect("registry");
        let endpoint = endpoint(6_881);
        let added = registry
            .observe(
                PeerObservation::new(endpoint, PeerSource::Incoming, false),
                Duration::ZERO,
            )
            .expect("incoming observation");
        assert_eq!(added.disposition, PeerObservationDisposition::Added);

        let merged = registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Tracker),
                Duration::from_secs(1),
            )
            .expect("tracker observation");
        assert_eq!(merged.record_id, added.record_id);
        assert_eq!(merged.disposition, PeerObservationDisposition::Merged);

        let record = registry.get(added.record_id).expect("merged record");
        assert!(record.is_connectable());
        assert!(record.sources().contains(PeerSource::Incoming));
        assert!(record.sources().contains(PeerSource::Tracker));
        assert_eq!(record.sources().len(), 2);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn retained_transfer_totals_accumulate_and_saturate() {
        let mut registry = PeerRegistry::new(config(1)).expect("registry");
        let record_id = registry
            .observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observation")
            .record_id;
        registry
            .record_transfer(
                record_id,
                PeerTransferTotals {
                    payload_downloaded_bytes: u64::MAX - 5,
                    payload_uploaded_bytes: u64::MAX - 7,
                },
            )
            .expect("initial transfer");
        registry
            .record_transfer(
                record_id,
                PeerTransferTotals {
                    payload_downloaded_bytes: 10,
                    payload_uploaded_bytes: 20,
                },
            )
            .expect("saturating transfer");

        assert_eq!(
            registry.get(record_id).expect("record").transfers(),
            PeerTransferTotals {
                payload_downloaded_bytes: u64::MAX,
                payload_uploaded_bytes: u64::MAX,
            }
        );
        assert_eq!(
            registry
                .snapshot(PeerSelectionContext {
                    now: Duration::ZERO
                })
                .records[0]
                .transfers,
            PeerTransferTotals {
                payload_downloaded_bytes: u64::MAX,
                payload_uploaded_bytes: u64::MAX,
            }
        );
    }

    #[test]
    fn utp_endpoint_memory_suppresses_retries_and_recovers() {
        let mut registry = PeerRegistry::new(config(4)).expect("registry");
        let endpoint = endpoint(6_881);
        let record_id = registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observation")
            .record_id;

        let first = PeerSelector
            .select(
                &registry,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("first candidate");
        let first = registry
            .begin_dial(
                first,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("first dial");
        assert_eq!(first.utp_decision(), UtpDialDecision::Try);
        registry
            .record_utp_outcome(first, Duration::ZERO, UtpConnectOutcome::Failed)
            .expect("first uTP failure");
        assert_eq!(
            registry
                .get(record_id)
                .expect("record")
                .history()
                .utp_endpoint,
            UtpEndpointState::Suppressed {
                failures: 1,
                retry_at: UTP_RETRY_INITIAL_BACKOFF,
            }
        );
        assert!(matches!(
            registry.record_utp_outcome(first, Duration::ZERO, UtpConnectOutcome::Failed),
            Err(PeerRegistryError::DuplicateUtpOutcome(_))
        ));
        registry
            .dial_succeeded(first, Duration::from_secs(1))
            .expect("fallback succeeded");
        registry
            .connection_closed(first, Duration::from_secs(2), None)
            .expect("fallback closed");

        let suppressed = PeerSelector
            .select(
                &registry,
                PeerSelectionContext {
                    now: Duration::from_secs(60),
                },
            )
            .expect("suppressed endpoint is still TCP eligible");
        let suppressed = registry
            .begin_dial(
                suppressed,
                PeerSelectionContext {
                    now: Duration::from_secs(60),
                },
            )
            .expect("suppressed dial");
        assert_eq!(
            suppressed.utp_decision(),
            UtpDialDecision::TcpWhileSuppressed
        );
        assert!(matches!(
            registry.record_utp_outcome(
                suppressed,
                Duration::from_secs(60),
                UtpConnectOutcome::Connected,
            ),
            Err(PeerRegistryError::UnexpectedUtpOutcome(_))
        ));
        registry
            .dial_cancelled(suppressed)
            .expect("cancel suppressed dial");

        let retry = PeerSelector
            .select(
                &registry,
                PeerSelectionContext {
                    now: UTP_RETRY_INITIAL_BACKOFF,
                },
            )
            .expect("retry candidate");
        let retry = registry
            .begin_dial(
                retry,
                PeerSelectionContext {
                    now: UTP_RETRY_INITIAL_BACKOFF,
                },
            )
            .expect("retry dial");
        assert_eq!(retry.utp_decision(), UtpDialDecision::Try);
        registry
            .record_utp_outcome(retry, UTP_RETRY_INITIAL_BACKOFF, UtpConnectOutcome::Failed)
            .expect("second uTP failure");
        assert_eq!(
            registry
                .get(record_id)
                .expect("record")
                .history()
                .utp_endpoint,
            UtpEndpointState::Suppressed {
                failures: 2,
                retry_at: UTP_RETRY_INITIAL_BACKOFF + UTP_RETRY_INITIAL_BACKOFF * 2,
            }
        );
        registry.dial_cancelled(retry).expect("cancel retry");

        registry
            .observe(
                PeerObservation::pex_dialable(endpoint, true),
                UTP_RETRY_INITIAL_BACKOFF + Duration::from_secs(1),
            )
            .expect("PEX refresh");
        assert_eq!(
            registry
                .get(record_id)
                .expect("record")
                .history()
                .utp_endpoint,
            UtpEndpointState::Advertised
        );
        let advertised = PeerSelector
            .select(
                &registry,
                PeerSelectionContext {
                    now: UTP_RETRY_INITIAL_BACKOFF + Duration::from_secs(1),
                },
            )
            .expect("advertised candidate");
        let advertised = registry
            .begin_dial(
                advertised,
                PeerSelectionContext {
                    now: UTP_RETRY_INITIAL_BACKOFF + Duration::from_secs(1),
                },
            )
            .expect("advertised dial");
        registry
            .record_utp_outcome(
                advertised,
                UTP_RETRY_INITIAL_BACKOFF + Duration::from_secs(2),
                UtpConnectOutcome::Connected,
            )
            .expect("uTP connected");
        assert_eq!(
            registry
                .get(record_id)
                .expect("record")
                .history()
                .utp_endpoint,
            UtpEndpointState::Confirmed
        );
    }

    #[test]
    fn utp_endpoint_memory_saturates_and_rejects_stale_or_overflowing_results() {
        let mut registry = PeerRegistry::new(config(2)).expect("registry");
        let endpoint = endpoint(6_881);
        let record_id = registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observation")
            .record_id;
        registry
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .expect("record")
            .history
            .utp_endpoint = UtpEndpointState::Suppressed {
            failures: u8::MAX,
            retry_at: Duration::ZERO,
        };
        let candidate = PeerSelector
            .select(
                &registry,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("candidate");
        let attempt = registry
            .begin_dial(
                candidate,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("dial");
        registry
            .record_utp_outcome(attempt, Duration::ZERO, UtpConnectOutcome::Failed)
            .expect("saturating failure");
        assert_eq!(
            registry
                .get(record_id)
                .expect("record")
                .history()
                .utp_endpoint,
            UtpEndpointState::Suppressed {
                failures: u8::MAX,
                retry_at: UTP_RETRY_MAX_BACKOFF,
            }
        );
        registry.dial_cancelled(attempt).expect("cancel dial");
        assert!(matches!(
            registry.record_utp_outcome(attempt, Duration::ZERO, UtpConnectOutcome::Connected),
            Err(PeerRegistryError::StaleAttempt(_))
        ));

        registry
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .expect("record")
            .history
            .utp_endpoint = UtpEndpointState::Unknown;
        let candidate = PeerSelector
            .select(&registry, PeerSelectionContext { now: Duration::MAX })
            .expect("candidate at maximum time");
        let attempt = registry
            .begin_dial(candidate, PeerSelectionContext { now: Duration::MAX })
            .expect("dial at maximum time");
        assert!(matches!(
            registry.record_utp_outcome(attempt, Duration::MAX, UtpConnectOutcome::Failed),
            Err(PeerRegistryError::TimeOverflow(id)) if id == record_id
        ));
        assert_eq!(
            registry
                .get(record_id)
                .expect("record")
                .history()
                .utp_endpoint,
            UtpEndpointState::Unknown
        );
    }

    #[test]
    fn snapshot_classifies_every_record_without_mutating_selection() {
        let mut registry = PeerRegistry::new(config(8)).expect("registry");
        let eligible = registry
            .observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("eligible observation")
            .record_id;
        registry
            .observe(
                PeerObservation::new(endpoint(6_882), PeerSource::Incoming, false),
                Duration::ZERO,
            )
            .expect("non-connectable observation");
        let candidate = PeerSelector
            .select(
                &registry,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("candidate");
        assert_eq!(candidate.record_id(), eligible);
        registry
            .begin_dial(
                candidate,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("begin dial");

        let snapshot = registry.snapshot(PeerSelectionContext {
            now: Duration::from_secs(1),
        });
        assert_eq!(snapshot.counts.total, 2);
        assert_eq!(snapshot.counts.dialing, 1);
        assert_eq!(snapshot.counts.not_connectable, 1);
        assert_eq!(snapshot.records.len(), 2);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn selection_tracks_attempt_backoff_success_close_and_stale_callbacks() {
        let mut registry = PeerRegistry::new(config(4)).expect("registry");
        let first = registry
            .observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::MagnetHint),
                Duration::ZERO,
            )
            .expect("observation")
            .record_id;
        let selector = PeerSelector;
        let initial = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = selector.select(&registry, initial).expect("candidate");
        assert_eq!(candidate.record_id(), first);
        let failed_attempt = registry
            .begin_dial(candidate, initial)
            .expect("begin failed attempt");
        registry
            .dial_failed(failed_attempt, Duration::from_secs(1), PeerFailure::Connect)
            .expect("record failure");
        assert!(matches!(
            registry.dial_succeeded(failed_attempt, Duration::from_secs(2)),
            Err(PeerRegistryError::StaleAttempt(_))
        ));

        let record = registry.get(first).expect("failed record");
        assert_eq!(
            selector.eligibility(
                record,
                PeerSelectionContext {
                    now: Duration::from_secs(10)
                },
                registry.config(),
            ),
            DialEligibility::Backoff {
                retry_at: Duration::from_secs(11)
            }
        );
        assert!(
            selector
                .select(
                    &registry,
                    PeerSelectionContext {
                        now: Duration::from_secs(11)
                    }
                )
                .is_some()
        );

        let retry_context = PeerSelectionContext {
            now: Duration::from_secs(11),
        };
        let retry = selector.select(&registry, retry_context).expect("retry");
        let successful_attempt = registry
            .begin_dial(retry, retry_context)
            .expect("begin successful attempt");
        registry
            .dial_succeeded(successful_attempt, Duration::from_secs(12))
            .expect("connect");
        assert!(matches!(
            registry.get(first).expect("connected record").phase(),
            PeerPhase::Connected { .. }
        ));
        assert!(matches!(
            registry.dial_failed(
                failed_attempt,
                Duration::from_secs(13),
                PeerFailure::Connect
            ),
            Err(PeerRegistryError::StaleAttempt(_))
        ));

        registry
            .connection_closed(successful_attempt, Duration::from_secs(14), None)
            .expect("clean close");
        let record = registry.get(first).expect("retained record");
        assert_eq!(record.phase(), PeerPhase::Idle);
        assert_eq!(record.history().dial_attempts, 2);
        assert_eq!(record.history().total_failures, 1);
        assert_eq!(
            record.history().last_disconnected_at,
            Some(Duration::from_secs(14))
        );
        assert!(matches!(
            registry.connection_closed(successful_attempt, Duration::from_secs(15), None),
            Err(PeerRegistryError::StaleAttempt(_))
        ));
    }

    #[test]
    fn integrity_reputation_is_asymmetric_bounded_and_generation_safe() {
        let mut registry = PeerRegistry::new(config(2)).expect("registry");
        let record_id = registry
            .observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observation")
            .record_id;
        let selector = PeerSelector;
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let attempt = registry
            .begin_dial(
                selector.select(&registry, context).expect("candidate"),
                context,
            )
            .expect("dial");
        registry
            .dial_succeeded(attempt, Duration::ZERO)
            .expect("connect");

        assert_eq!(
            registry
                .record_piece_failed(attempt, false)
                .expect("ambiguous failure"),
            PeerIntegrityAction::Retain
        );
        let failed = registry.get(record_id).expect("record").integrity();
        assert_eq!(failed.trust_points, -2);
        assert_eq!(failed.hash_failures, 1);
        assert!(failed.on_parole);

        registry.record_piece_passed(attempt).expect("valid piece");
        let passed = registry.get(record_id).expect("record").integrity();
        assert_eq!(passed.trust_points, -1);
        assert_eq!(passed.valid_pieces, 1);
        assert!(!passed.on_parole);

        registry
            .connection_closed(attempt, Duration::from_secs(1), None)
            .expect("close");
        let retry_context = PeerSelectionContext {
            now: Duration::from_secs(1),
        };
        let retry = registry
            .begin_dial(
                selector
                    .select(&registry, retry_context)
                    .expect("retry candidate"),
                retry_context,
            )
            .expect("retry dial");
        registry
            .dial_succeeded(retry, Duration::from_secs(1))
            .expect("retry connect");
        assert!(matches!(
            registry.record_piece_failed(attempt, true),
            Err(PeerRegistryError::StaleAttempt(_))
        ));
        assert_eq!(
            registry
                .record_piece_failed(retry, true)
                .expect("known bad"),
            PeerIntegrityAction::Ban
        );
        let known_bad = registry.get(record_id).expect("record").integrity();
        assert_eq!(known_bad.trust_points, -3);
        assert_eq!(known_bad.hash_failures, 2);
        assert!(known_bad.on_parole);
    }

    #[test]
    fn known_bad_incoming_contributor_is_banned_after_disconnect() {
        let mut registry = PeerRegistry::new(config(1)).expect("registry");
        let record_id = registry
            .observe(
                PeerObservation::new(endpoint(6_881), PeerSource::Incoming, false),
                Duration::ZERO,
            )
            .expect("incoming observation")
            .record_id;
        registry
            .incoming_connected(record_id, Duration::ZERO)
            .expect("incoming connected");

        assert_eq!(
            registry
                .record_incoming_piece_failed(record_id, true)
                .expect("known bad incoming piece"),
            PeerIntegrityAction::Ban
        );
        assert_eq!(
            PeerSelector.eligibility(
                registry.get(record_id).expect("active incoming record"),
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
                config(1),
            ),
            DialEligibility::Connected
        );

        registry
            .incoming_closed(record_id, Duration::from_secs(1), None)
            .expect("incoming closed");
        assert_eq!(
            registry.get(record_id).expect("banned record").phase(),
            PeerPhase::Banned
        );
    }

    #[test]
    fn repeated_ambiguous_failures_reach_the_fixed_ban_floor() {
        let mut registry = PeerRegistry::new(config(1)).expect("registry");
        registry
            .observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observation");
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let attempt = registry
            .begin_dial(
                PeerSelector.select(&registry, context).expect("candidate"),
                context,
            )
            .expect("dial");
        registry
            .dial_succeeded(attempt, Duration::ZERO)
            .expect("connect");
        for _ in 0..3 {
            assert_eq!(
                registry
                    .record_piece_failed(attempt, false)
                    .expect("ambiguous failure"),
                PeerIntegrityAction::Retain
            );
        }
        assert_eq!(
            registry
                .record_piece_failed(attempt, false)
                .expect("trust floor"),
            PeerIntegrityAction::Ban
        );
        assert_eq!(
            registry
                .records()
                .next()
                .expect("record")
                .integrity()
                .trust_points,
            -7
        );
    }

    #[test]
    fn cancelled_dial_returns_idle_without_recording_a_failure() {
        let mut registry = PeerRegistry::new(config(1)).expect("registry");
        registry
            .observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::Manual),
                Duration::ZERO,
            )
            .expect("observation");
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = PeerSelector.select(&registry, context).expect("candidate");
        let attempt = registry.begin_dial(candidate, context).expect("dial");
        registry.dial_cancelled(attempt).expect("cancel dial");
        let record = registry
            .find_endpoint(endpoint(6_881))
            .expect("retained record");
        assert_eq!(record.phase(), PeerPhase::Idle);
        assert_eq!(record.history().dial_attempts, 1);
        assert_eq!(record.history().total_failures, 0);
        assert_eq!(record.history().consecutive_failures, 0);
        assert!(PeerSelector.select(&registry, context).is_some());
    }

    #[test]
    fn bounded_registry_prunes_failed_idle_records_but_not_active_records() {
        assert!(matches!(
            PeerRegistry::new(PeerRegistryConfig {
                max_records: 0,
                ..config(1)
            }),
            Err(PeerRegistryError::ZeroCapacity)
        ));
        let mut registry = PeerRegistry::new(config(2)).expect("registry");
        let failed = registry
            .observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::MagnetHint),
                Duration::ZERO,
            )
            .expect("failed observation")
            .record_id;
        let healthy = registry
            .observe(
                PeerObservation::dialable(endpoint(6_882), PeerSource::MagnetHint),
                Duration::from_secs(1),
            )
            .expect("healthy observation")
            .record_id;
        let selector = PeerSelector;
        let candidate = selector
            .select(
                &registry,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("first candidate");
        let attempt = registry
            .begin_dial(
                candidate,
                PeerSelectionContext {
                    now: Duration::ZERO,
                },
            )
            .expect("attempt");
        registry
            .dial_failed(attempt, Duration::ZERO, PeerFailure::Connect)
            .expect("failure");

        let replacement = registry
            .observe(
                PeerObservation::dialable(endpoint(6_883), PeerSource::Tracker),
                Duration::from_secs(2),
            )
            .expect("replacement");
        assert_eq!(
            replacement.disposition,
            PeerObservationDisposition::Replaced { evicted: failed }
        );
        assert!(registry.get(failed).is_none());
        assert!(registry.get(healthy).is_some());

        let active_candidate = selector
            .select(
                &registry,
                PeerSelectionContext {
                    now: Duration::from_secs(2),
                },
            )
            .expect("active candidate");
        registry
            .begin_dial(
                active_candidate,
                PeerSelectionContext {
                    now: Duration::from_secs(2),
                },
            )
            .expect("active attempt");
        let other_id = registry
            .records()
            .find(|record| record.phase() == PeerPhase::Idle)
            .expect("other idle record")
            .id();
        registry.ban(other_id).expect("ban other record");

        assert!(matches!(
            registry.observe(
                PeerObservation::dialable(endpoint(6_884), PeerSource::Tracker),
                Duration::from_secs(3)
            ),
            Err(PeerRegistryError::CapacityExhausted { maximum: 2 })
        ));
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn identifier_and_history_counters_fail_closed() {
        let mut record_overflow = PeerRegistry::new(config(1)).expect("registry");
        record_overflow.next_record_id = u64::MAX;
        assert!(matches!(
            record_overflow.observe(
                PeerObservation::dialable(endpoint(6_881), PeerSource::Tracker),
                Duration::ZERO
            ),
            Err(PeerRegistryError::IdentifierOverflow("peer record"))
        ));
        assert!(record_overflow.is_empty());

        let mut attempt_overflow = PeerRegistry::new(config(1)).expect("registry");
        let record_id = attempt_overflow
            .observe(
                PeerObservation::dialable(endpoint(6_882), PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observation")
            .record_id;
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = PeerSelector
            .select(&attempt_overflow, context)
            .expect("candidate");
        attempt_overflow.next_attempt_id = u64::MAX;
        assert!(matches!(
            attempt_overflow.begin_dial(candidate, context),
            Err(PeerRegistryError::IdentifierOverflow("dial attempt"))
        ));
        assert_eq!(
            attempt_overflow.get(record_id).expect("record").phase(),
            PeerPhase::Idle
        );
        assert_eq!(
            attempt_overflow
                .get(record_id)
                .expect("record")
                .history()
                .dial_attempts,
            0
        );

        attempt_overflow.next_attempt_id = 1;
        attempt_overflow
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .expect("record")
            .history
            .dial_attempts = u32::MAX;
        assert!(matches!(
            attempt_overflow.begin_dial(candidate, context),
            Err(PeerRegistryError::HistoryOverflow(id)) if id == record_id
        ));
        assert_eq!(
            attempt_overflow.get(record_id).expect("record").phase(),
            PeerPhase::Idle
        );
    }
}

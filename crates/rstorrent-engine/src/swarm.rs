//! Runtime-independent multi-peer request ownership and scheduling.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::time::Duration;

use rstorrent_protocol::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH, PeerMessage};
use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;

use crate::piece_picker::{AvailabilityPicker, PieceActivationPolicy};

pub const DEFAULT_MAX_ESTABLISHED_CONNECTIONS: usize = 30;
pub const DEFAULT_MAX_PENDING_DIALS: usize = 30;
pub const DEFAULT_INITIAL_REQUESTS_PER_CONNECTION: usize = 4;
pub const DEFAULT_MAX_REQUESTS_PER_CONNECTION: usize = 500;
pub const DEFAULT_MAX_ACTIVE_PIECE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_MAX_ACTIVE_PIECES: usize = 2_048;
pub const DEFAULT_MAX_TERMINAL_ATTEMPTS_PER_BLOCK: usize = DEFAULT_MAX_ESTABLISHED_CONNECTIONS;
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_UNPRODUCTIVE_GRACE: Duration = Duration::from_secs(60);
pub const DEFAULT_REQUEST_QUEUE_TIME: Duration = Duration::from_secs(3);
pub const MAX_FAST_ADVISORY_PIECES: usize = 32;

const MIN_HEALTHY_REQUESTS_PER_CONNECTION: usize = 2;
const REQUEST_RATE_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const SLOW_START_RATE_SLACK_BYTES: usize = 5_000;
const MAX_REQUEST_TIME_SAMPLES: usize = 20;
const MIN_ADAPTIVE_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnectionId(u64);

impl ConnectionId {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PendingDialId(u64);

impl PendingDialId {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestAttemptId(u64);

impl RequestAttemptId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PieceGeneration(u64);

impl PieceGeneration {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockKey {
    pub piece: u32,
    pub begin: u32,
    pub length: u32,
}

impl BlockKey {
    pub fn new(piece: u32, begin: u32, length: u32) -> Result<Self, SwarmError> {
        if length == 0 || length > MAX_REQUEST_BLOCK_LENGTH || begin.checked_add(length).is_none() {
            return Err(SwarmError::InvalidBlock {
                piece,
                begin,
                length,
            });
        }
        Ok(Self {
            piece,
            begin,
            length,
        })
    }

    pub const fn request(self) -> BlockRequest {
        BlockRequest {
            index: self.piece,
            begin: self.begin,
            length: self.length,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PiecePlan {
    index: u32,
    blocks: Vec<BlockKey>,
}

impl PiecePlan {
    pub fn new(index: u32, ranges: &[(u32, u32)]) -> Result<Self, SwarmError> {
        if ranges.is_empty() {
            return Err(SwarmError::EmptyPiecePlan(index));
        }
        let mut blocks = ranges
            .iter()
            .map(|&(begin, length)| BlockKey::new(index, begin, length))
            .collect::<Result<Vec<_>, _>>()?;
        blocks.sort_unstable_by_key(|block| block.begin);
        let mut previous_end = 0;
        for block in &blocks {
            if block.begin < previous_end {
                return Err(SwarmError::OverlappingBlocks(index));
            }
            previous_end =
                block
                    .begin
                    .checked_add(block.length)
                    .ok_or(SwarmError::InvalidBlock {
                        piece: block.piece,
                        begin: block.begin,
                        length: block.length,
                    })?;
        }
        Ok(Self { index, blocks })
    }

    pub const fn index(&self) -> u32 {
        self.index
    }

    pub fn blocks(&self) -> &[BlockKey] {
        &self.blocks
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwarmConfig {
    pub max_established_connections: usize,
    pub max_pending_dials: usize,
    pub initial_requests_per_connection: usize,
    pub max_requests_per_connection: usize,
    pub max_active_piece_bytes: usize,
    pub max_active_pieces: usize,
    pub piece_activation_policy: PieceActivationPolicy,
    pub max_terminal_attempts_per_block: usize,
    pub max_outstanding_request_bytes: usize,
    pub request_timeout: Duration,
    pub request_queue_time: Duration,
    pub unproductive_grace: Duration,
}

impl SwarmConfig {
    pub const fn for_request_limit(max_outstanding_request_bytes: usize) -> Self {
        Self {
            max_established_connections: DEFAULT_MAX_ESTABLISHED_CONNECTIONS,
            max_pending_dials: DEFAULT_MAX_PENDING_DIALS,
            initial_requests_per_connection: DEFAULT_INITIAL_REQUESTS_PER_CONNECTION,
            max_requests_per_connection: DEFAULT_MAX_REQUESTS_PER_CONNECTION,
            max_active_piece_bytes: DEFAULT_MAX_ACTIVE_PIECE_BYTES,
            max_active_pieces: DEFAULT_MAX_ACTIVE_PIECES,
            piece_activation_policy: PieceActivationPolicy::RarestFirst,
            max_terminal_attempts_per_block: DEFAULT_MAX_TERMINAL_ATTEMPTS_PER_BLOCK,
            max_outstanding_request_bytes,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            request_queue_time: DEFAULT_REQUEST_QUEUE_TIME,
            unproductive_grace: DEFAULT_UNPRODUCTIVE_GRACE,
        }
    }

    fn validate(self) -> Result<Self, SwarmError> {
        if self.max_established_connections == 0 {
            return Err(SwarmError::InvalidConfig(
                "established connection limit must be nonzero",
            ));
        }
        if self.max_pending_dials == 0 {
            return Err(SwarmError::InvalidConfig(
                "pending dial limit must be nonzero",
            ));
        }
        if self.max_requests_per_connection < MIN_HEALTHY_REQUESTS_PER_CONNECTION {
            return Err(SwarmError::InvalidConfig(
                "per-connection request limit must permit a healthy window",
            ));
        }
        if self.initial_requests_per_connection < MIN_HEALTHY_REQUESTS_PER_CONNECTION
            || self.initial_requests_per_connection > self.max_requests_per_connection
        {
            return Err(SwarmError::InvalidConfig(
                "initial request window must be healthy and within the connection limit",
            ));
        }
        if self.max_active_piece_bytes < MIN_PAYLOAD_ALLOWANCE {
            return Err(SwarmError::InvalidConfig(
                "active piece working set is smaller than one request block",
            ));
        }
        if self.max_active_pieces == 0 {
            return Err(SwarmError::InvalidConfig(
                "active piece count limit must be nonzero",
            ));
        }
        if self.max_established_connections > u16::MAX as usize {
            return Err(SwarmError::InvalidConfig(
                "established connection limit exceeds availability counter width",
            ));
        }
        if self.max_terminal_attempts_per_block == 0 {
            return Err(SwarmError::InvalidConfig(
                "terminal attempt retention must be nonzero",
            ));
        }
        if self.max_outstanding_request_bytes < MIN_PAYLOAD_ALLOWANCE {
            return Err(SwarmError::InvalidConfig(
                "outstanding request limit is smaller than one request block",
            ));
        }
        if self.request_timeout.is_zero()
            || self.request_queue_time.is_zero()
            || self.unproductive_grace.is_zero()
        {
            return Err(SwarmError::InvalidConfig(
                "request queue, request, and unproductive times must be nonzero",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestDisposition {
    Requested,
    PayloadReceived,
    Expired,
    Choked,
    Rejected,
    Disconnected,
    Superseded,
    Cancelled,
}

impl RequestDisposition {
    const fn is_active(self) -> bool {
        matches!(self, Self::Requested)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestAttempt {
    pub id: RequestAttemptId,
    pub block: BlockKey,
    pub connection: ConnectionId,
    pub issued_at: Duration,
    pub disposition: RequestDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestAssignment {
    pub attempt: RequestAttemptId,
    pub connection: ConnectionId,
    pub block: BlockKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpiredRequest {
    pub attempt: RequestAttemptId,
    pub connection: ConnectionId,
    pub block: BlockKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestCancellation {
    pub attempt: RequestAttemptId,
    pub connection: ConnectionId,
    pub block: BlockKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectDisposition {
    Accepted { attempt: RequestAttemptId },
    NeverRequested,
    Stale,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FastPeerSnapshot {
    pub negotiated: bool,
    pub initial_availability_received: bool,
    pub suggestions: usize,
    pub allowed_fast: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PieceHashFailure {
    pub piece: u32,
    pub contributors: Vec<ConnectionId>,
    pub failed_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReceiveDisposition {
    Accept {
        evidence: RequestAttemptId,
        cancellations: Vec<RequestCancellation>,
        late: bool,
    },
    Redundant,
    Unsolicited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockStatus {
    Missing,
    Requested,
    Writing,
    Received,
    Verified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoRequestReason {
    Complete,
    NoConnections,
    AllUsefulPeersChoked,
    NoPeerHasWantedPiece,
    OutstandingRequestLimit,
    RequestWindowsFull,
    ActivePieceLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwarmSnapshot {
    pub pending_dials: usize,
    pub connected_peers: usize,
    pub unchoked_peers: usize,
    pub missing_blocks: usize,
    pub requested_blocks: usize,
    pub active_request_attempts: usize,
    pub active_duplicate_attempts: usize,
    pub writing_blocks: usize,
    pub received_blocks: usize,
    pub verified_blocks: usize,
    pub active_piece_count: usize,
    pub active_piece_bytes: usize,
    pub max_active_pieces: usize,
    pub piece_activation_policy: PieceActivationPolicy,
    pub availability_seed_count: usize,
    pub picker_retained_bytes: usize,
    pub picker_rank_comparisons: u64,
    pub picker_bulk_rebuilds: u64,
    pub picker_candidate_inspections: u64,
    pub active_piece_visits: u64,
    pub inactive_planned_piece_visits: u64,
    pub last_activated_piece: Option<u32>,
    pub last_activated_availability: Option<u32>,
    pub outstanding_request_bytes: usize,
    pub outstanding_request_high_water: usize,
    pub request_target_total: usize,
    pub request_target_max: usize,
    pub slow_start_peers: usize,
    pub stalled_peers: usize,
    pub useful_payload_bytes: usize,
    pub observed_payload_rate: usize,
    pub endgame_assignments: usize,
    pub cancelled_request_attempts: usize,
    pub redundant_payload_bytes: usize,
    pub piece_hash_failures: usize,
    pub failed_piece_bytes: usize,
    pub last_hash_failure_contributors: usize,
    pub request_timeout_min: Option<Duration>,
    pub request_timeout_max: Option<Duration>,
    pub oldest_request_age: Option<Duration>,
    pub next_request_expiry: Option<Duration>,
    pub next_replacement_at: Option<Duration>,
    pub no_request_reason: Option<NoRequestReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionWindowPhaseSnapshot {
    SlowStart,
    Steady,
    Stalled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConnectionActivitySnapshot {
    pub id: ConnectionId,
    pub choking: bool,
    pub wanted_piece_count: usize,
    pub pending_requests: usize,
    pub target_requests: usize,
    pub queued_payload_bytes: usize,
    pub window_phase: ConnectionWindowPhaseSnapshot,
    pub useful_payload_bytes: usize,
    pub observed_payload_rate: usize,
    pub connected_age: Duration,
    pub last_useful_age: Option<Duration>,
    pub last_payload_age: Option<Duration>,
    pub request_timeout: Duration,
    pub oldest_request_age: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionRemoval {
    Disconnected,
    Replaced,
    Cancelled,
}

#[derive(Debug)]
struct ConnectionState {
    availability: PieceAvailability,
    wanted_piece_count: usize,
    choking: bool,
    fast_extension: bool,
    initial_availability_received: bool,
    suggestions: VecDeque<u32>,
    allowed_fast: BTreeSet<u32>,
    connected_at: Duration,
    last_useful_at: Option<Duration>,
    active_request_count: usize,
    request_window: RequestWindow,
}

impl ConnectionState {
    fn can_request_piece(&self, piece: usize) -> bool {
        self.availability.contains(piece)
            && (!self.choking
                || self.fast_extension
                    && u32::try_from(piece).is_ok_and(|piece| self.allowed_fast.contains(&piece)))
    }

    fn has_choked_fast_work(&self) -> bool {
        self.fast_extension && self.choking && !self.allowed_fast.is_empty()
    }
}

#[derive(Debug)]
struct PieceAvailability {
    bytes: Vec<u8>,
    piece_count: usize,
    set_count: usize,
}

impl PieceAvailability {
    fn empty(piece_count: usize) -> Self {
        Self {
            bytes: vec![0; piece_count.div_ceil(8)],
            piece_count,
            set_count: 0,
        }
    }

    fn full(piece_count: usize) -> Self {
        let mut bytes = vec![0xff; piece_count.div_ceil(8)];
        let remainder = piece_count % 8;
        if remainder != 0 {
            bytes[piece_count / 8] &= 0xff << (8 - remainder);
        }
        Self {
            bytes,
            piece_count,
            set_count: piece_count,
        }
    }

    fn from_flags(flags: Vec<bool>) -> Self {
        let mut availability = Self::empty(flags.len());
        for (piece, available) in flags.into_iter().enumerate() {
            if available {
                availability.set(piece);
            }
        }
        availability
    }

    fn from_bytes(bytes: Vec<u8>, piece_count: usize) -> Option<Self> {
        if bytes.len() != piece_count.div_ceil(8) {
            return None;
        }
        let remainder = piece_count % 8;
        if remainder != 0 {
            let unused_mask = (1_u8 << (8 - remainder)) - 1;
            if bytes.last().is_some_and(|byte| byte & unused_mask != 0) {
                return None;
            }
        }
        let set_count = bytes.iter().map(|byte| byte.count_ones() as usize).sum();
        Some(Self {
            bytes,
            piece_count,
            set_count,
        })
    }

    fn contains(&self, piece: usize) -> bool {
        debug_assert!(piece < self.piece_count);
        self.bytes[piece / 8] & (1 << (7 - piece % 8)) != 0
    }

    fn set(&mut self, piece: usize) -> bool {
        debug_assert!(piece < self.piece_count);
        let mask = 1 << (7 - piece % 8);
        let byte = &mut self.bytes[piece / 8];
        if *byte & mask != 0 {
            return false;
        }
        *byte |= mask;
        self.set_count += 1;
        true
    }

    fn is_full(&self) -> bool {
        self.set_count == self.piece_count
    }

    fn set_pieces(&self) -> impl Iterator<Item = usize> + '_ {
        let piece_count = self.piece_count;
        self.bytes
            .iter()
            .enumerate()
            .flat_map(move |(byte_index, byte)| {
                (0..8).filter_map(move |bit| {
                    let piece = byte_index * 8 + bit;
                    (piece < piece_count && byte & (1 << (7 - bit)) != 0).then_some(piece)
                })
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestWindowPhase {
    SlowStart,
    Steady,
    Stalled,
}

#[derive(Debug)]
struct RequestWindow {
    target: usize,
    phase: RequestWindowPhase,
    sample_started_at: Duration,
    sample_payload_bytes: usize,
    previous_payload_rate: Option<usize>,
    observed_payload_rate: usize,
    useful_payload_bytes: usize,
    last_payload_at: Option<Duration>,
    request_time_samples: VecDeque<Duration>,
}

impl RequestWindow {
    fn new(now: Duration, initial_target: usize) -> Self {
        Self {
            target: initial_target,
            phase: RequestWindowPhase::SlowStart,
            sample_started_at: now,
            sample_payload_bytes: 0,
            previous_payload_rate: None,
            observed_payload_rate: 0,
            useful_payload_bytes: 0,
            last_payload_at: None,
            request_time_samples: VecDeque::new(),
        }
    }

    fn refresh(&mut self, now: Duration, config: SwarmConfig) {
        let elapsed = now.saturating_sub(self.sample_started_at);
        if elapsed < REQUEST_RATE_SAMPLE_INTERVAL {
            return;
        }
        let elapsed_millis = elapsed.as_millis().max(1);
        let rate = ((self.sample_payload_bytes as u128)
            .saturating_mul(1_000)
            .checked_div(elapsed_millis)
            .unwrap_or(0))
        .min(usize::MAX as u128) as usize;
        self.observed_payload_rate = rate;

        if self.phase == RequestWindowPhase::SlowStart {
            if self.previous_payload_rate.is_some_and(|previous| {
                previous > 0 && rate <= previous.saturating_add(SLOW_START_RATE_SLACK_BYTES)
            }) {
                self.phase = RequestWindowPhase::Steady;
            }
            self.previous_payload_rate = Some(rate);
        }
        if self.phase == RequestWindowPhase::Steady {
            self.target = rate_target(rate, config);
        }
        self.sample_started_at = now;
        self.sample_payload_bytes = 0;
    }

    fn accepted_payload(
        &mut self,
        now: Duration,
        length: usize,
        request_issued_at: Option<Duration>,
        config: SwarmConfig,
    ) {
        self.refresh(now, config);
        if let Some(issued_at) = request_issued_at {
            let response_started_at = self.last_payload_at.unwrap_or(issued_at).max(issued_at);
            self.record_request_time(now.saturating_sub(response_started_at));
        }
        self.last_payload_at = Some(now);
        self.sample_payload_bytes = self.sample_payload_bytes.saturating_add(length);
        self.useful_payload_bytes = self.useful_payload_bytes.saturating_add(length);
        match self.phase {
            RequestWindowPhase::SlowStart => {
                self.target = self
                    .target
                    .saturating_add(1)
                    .min(config.max_requests_per_connection);
            }
            RequestWindowPhase::Steady => {}
            RequestWindowPhase::Stalled => {
                self.phase = RequestWindowPhase::Steady;
                self.target = MIN_HEALTHY_REQUESTS_PER_CONNECTION;
                self.previous_payload_rate = None;
            }
        }
    }

    fn stalled(&mut self, now: Duration) {
        self.target = 1;
        self.phase = RequestWindowPhase::Stalled;
        self.sample_started_at = now;
        self.sample_payload_bytes = 0;
        self.previous_payload_rate = None;
        self.observed_payload_rate = 0;
        self.last_payload_at = None;
    }

    fn record_request_time(&mut self, request_time: Duration) {
        if self.request_time_samples.len() == MAX_REQUEST_TIME_SAMPLES {
            self.request_time_samples.pop_front();
        }
        self.request_time_samples.push_back(request_time);
    }

    fn request_timeout(&self, config: SwarmConfig) -> Duration {
        if self.request_time_samples.is_empty() {
            return config.request_timeout;
        }
        let sample_count = self.request_time_samples.len() as u128;
        let total_millis = self
            .request_time_samples
            .iter()
            .map(Duration::as_millis)
            .fold(0_u128, u128::saturating_add);
        let average = total_millis / sample_count;
        let timeout_millis = if self.request_time_samples.len() == 1 {
            average.saturating_add(average / 5)
        } else {
            let deviation = self
                .request_time_samples
                .iter()
                .map(|sample| sample.as_millis().abs_diff(average))
                .fold(0_u128, u128::saturating_add)
                / sample_count;
            average.saturating_add(deviation.saturating_mul(4))
        };
        let rounded_millis = timeout_millis
            .saturating_add(999)
            .checked_div(1_000)
            .unwrap_or(0)
            .saturating_mul(1_000);
        let maximum = config.request_timeout.as_millis();
        let minimum = MIN_ADAPTIVE_REQUEST_TIMEOUT.as_millis().min(maximum);
        let bounded = rounded_millis.clamp(minimum, maximum);
        Duration::from_millis(u64::try_from(bounded).unwrap_or(u64::MAX))
    }

    fn stall_deadline(&self, oldest_request: Duration, config: SwarmConfig) -> Duration {
        self.last_payload_at
            .unwrap_or(oldest_request)
            .checked_add(self.request_timeout(config))
            .unwrap_or(Duration::MAX)
    }
}

fn rate_target(payload_rate: usize, config: SwarmConfig) -> usize {
    let queue_millis = config.request_queue_time.as_millis();
    let target_bytes = (payload_rate as u128)
        .saturating_mul(queue_millis)
        .checked_div(1_000)
        .unwrap_or(0);
    let block_length = MAX_REQUEST_BLOCK_LENGTH as u128;
    let requests = target_bytes
        .saturating_add(block_length.saturating_sub(1))
        .checked_div(block_length)
        .unwrap_or(0)
        .min(usize::MAX as u128) as usize;
    requests.clamp(
        MIN_HEALTHY_REQUESTS_PER_CONNECTION,
        config.max_requests_per_connection,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockPhase {
    Missing,
    Requested,
    Writing {
        source: ConnectionId,
        evidence: RequestAttemptId,
    },
    Received {
        source: ConnectionId,
        evidence: RequestAttemptId,
    },
    Verified,
}

impl BlockPhase {
    const fn status(self) -> BlockStatus {
        match self {
            Self::Missing => BlockStatus::Missing,
            Self::Requested => BlockStatus::Requested,
            Self::Writing { .. } => BlockStatus::Writing,
            Self::Received { .. } => BlockStatus::Received,
            Self::Verified => BlockStatus::Verified,
        }
    }
}

#[derive(Debug)]
struct BlockState {
    phase: BlockPhase,
    attempts: VecDeque<RequestAttempt>,
}

#[derive(Debug)]
struct PieceState {
    blocks: Vec<BlockKey>,
    working_set_bytes: usize,
    missing_blocks: usize,
    active_blocks: usize,
    first_missing_block: usize,
    storage: PieceStorageJoin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PieceHashJoinState {
    NotStarted,
    Running,
    Passed,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PieceStorageOutcome {
    Pending,
    Verified,
    Failed,
}

#[derive(Debug)]
struct PieceStorageJoin {
    generation: PieceGeneration,
    writes_expected: usize,
    writes_completed: usize,
    write_failed: bool,
    hash: PieceHashJoinState,
}

impl PieceStorageJoin {
    fn new(writes_expected: usize) -> Self {
        debug_assert_ne!(writes_expected, 0);
        Self {
            generation: PieceGeneration(1),
            writes_expected,
            writes_completed: 0,
            write_failed: false,
            hash: PieceHashJoinState::NotStarted,
        }
    }

    fn validate_generation(&self, generation: PieceGeneration) -> Result<(), SwarmError> {
        if generation == self.generation {
            Ok(())
        } else {
            Err(SwarmError::StalePieceGeneration {
                expected: self.generation,
                actual: generation,
            })
        }
    }

    fn write_completed(
        &mut self,
        generation: PieceGeneration,
        succeeded: bool,
    ) -> Result<PieceStorageOutcome, SwarmError> {
        self.validate_generation(generation)?;
        if self.writes_completed == self.writes_expected {
            return Err(SwarmError::Invariant(
                "piece received more write completions than expected",
            ));
        }
        self.writes_completed = self
            .writes_completed
            .checked_add(1)
            .ok_or(SwarmError::ArithmeticOverflow("piece write completions"))?;
        self.write_failed |= !succeeded;
        Ok(self.outcome())
    }

    fn hash_eligible(&self, generation: PieceGeneration) -> Result<bool, SwarmError> {
        self.validate_generation(generation)?;
        Ok(self.writes_completed == self.writes_expected
            && !self.write_failed
            && self.hash == PieceHashJoinState::NotStarted)
    }

    fn begin_hash(&mut self, generation: PieceGeneration) -> Result<(), SwarmError> {
        self.validate_generation(generation)?;
        if self.hash != PieceHashJoinState::NotStarted {
            return Err(SwarmError::InvalidTransition(
                "piece hash is already started or completed",
            ));
        }
        self.hash = PieceHashJoinState::Running;
        Ok(())
    }

    fn hash_completed(
        &mut self,
        generation: PieceGeneration,
        passed: bool,
    ) -> Result<PieceStorageOutcome, SwarmError> {
        self.validate_generation(generation)?;
        if self.hash != PieceHashJoinState::Running {
            return Err(SwarmError::InvalidTransition(
                "piece hash completion has no running hash",
            ));
        }
        self.hash = if passed {
            PieceHashJoinState::Passed
        } else {
            PieceHashJoinState::Failed
        };
        Ok(self.outcome())
    }

    fn outcome(&self) -> PieceStorageOutcome {
        if self.writes_completed != self.writes_expected {
            return PieceStorageOutcome::Pending;
        }
        if self.write_failed || self.hash == PieceHashJoinState::Failed {
            return PieceStorageOutcome::Failed;
        }
        if self.hash == PieceHashJoinState::Passed {
            PieceStorageOutcome::Verified
        } else {
            PieceStorageOutcome::Pending
        }
    }

    fn reset(&mut self) -> Result<(), SwarmError> {
        let generation = self
            .generation
            .0
            .checked_add(1)
            .map(PieceGeneration)
            .ok_or(SwarmError::IdentifierOverflow("piece generation"))?;
        *self = Self {
            generation,
            writes_expected: self.writes_expected,
            writes_completed: 0,
            write_failed: false,
            hash: PieceHashJoinState::NotStarted,
        };
        Ok(())
    }
}

#[derive(Debug)]
pub struct SwarmState {
    config: SwarmConfig,
    piece_count: usize,
    picker: AvailabilityPicker,
    pieces: BTreeMap<u32, PieceState>,
    blocks: BTreeMap<BlockKey, BlockState>,
    incomplete_pieces: BTreeSet<usize>,
    inactive_ranked_pieces: Vec<u32>,
    inactive_rank_dirty: bool,
    active_pieces: BTreeSet<usize>,
    requestable_active_pieces: BTreeSet<usize>,
    requestable_active_block_keys: BTreeMap<usize, BlockKey>,
    requestable_active_blocks: usize,
    active_piece_bytes: usize,
    active_attempts: BTreeMap<RequestAttemptId, RequestAttempt>,
    unverified_contributor_blocks: BTreeMap<ConnectionId, usize>,
    connections: BTreeMap<ConnectionId, ConnectionState>,
    pending_dials: BTreeSet<PendingDialId>,
    next_attempt_id: u64,
    missing_blocks: usize,
    requested_blocks: usize,
    writing_blocks: usize,
    received_blocks: usize,
    verified_blocks: usize,
    outstanding_request_bytes: usize,
    outstanding_request_high_water: usize,
    last_scheduled_connection: Option<ConnectionId>,
    last_activated_piece: Option<u32>,
    last_activated_availability: Option<u32>,
    active_piece_visits: u64,
    inactive_planned_piece_visits: u64,
    endgame_assignments: usize,
    cancelled_request_attempts: usize,
    redundant_payload_bytes: usize,
    piece_hash_failures: usize,
    failed_piece_bytes: usize,
    last_hash_failure_contributors: usize,
}

impl SwarmState {
    pub fn new(
        config: SwarmConfig,
        piece_count: usize,
        plans: Vec<PiecePlan>,
    ) -> Result<Self, SwarmError> {
        if plans.is_empty() {
            return Err(SwarmError::InvalidConfig(
                "at least one wanted piece plan is required",
            ));
        }
        let wanted = plans.iter().map(PiecePlan::index).collect();
        Self::new_with_wanted(config, piece_count, wanted, plans, 0)
    }

    pub fn new_with_wanted(
        config: SwarmConfig,
        piece_count: usize,
        wanted: Vec<u32>,
        plans: Vec<PiecePlan>,
        picker_seed: u64,
    ) -> Result<Self, SwarmError> {
        let config = config.validate()?;
        if piece_count == 0 {
            return Err(SwarmError::InvalidConfig("piece count must be nonzero"));
        }
        if wanted.is_empty() {
            return Err(SwarmError::InvalidConfig(
                "at least one wanted piece is required",
            ));
        }
        let picker = AvailabilityPicker::new(
            piece_count,
            wanted,
            config.piece_activation_policy,
            picker_seed,
        )
        .map_err(SwarmError::Invariant)?;
        let mut state = Self {
            config,
            piece_count,
            picker,
            pieces: BTreeMap::new(),
            blocks: BTreeMap::new(),
            incomplete_pieces: BTreeSet::new(),
            inactive_ranked_pieces: Vec::new(),
            inactive_rank_dirty: false,
            active_pieces: BTreeSet::new(),
            requestable_active_pieces: BTreeSet::new(),
            requestable_active_block_keys: BTreeMap::new(),
            requestable_active_blocks: 0,
            active_piece_bytes: 0,
            active_attempts: BTreeMap::new(),
            unverified_contributor_blocks: BTreeMap::new(),
            connections: BTreeMap::new(),
            pending_dials: BTreeSet::new(),
            next_attempt_id: 1,
            missing_blocks: 0,
            requested_blocks: 0,
            writing_blocks: 0,
            received_blocks: 0,
            verified_blocks: 0,
            outstanding_request_bytes: 0,
            outstanding_request_high_water: 0,
            last_scheduled_connection: None,
            last_activated_piece: None,
            last_activated_availability: None,
            active_piece_visits: 0,
            inactive_planned_piece_visits: 0,
            endgame_assignments: 0,
            cancelled_request_attempts: 0,
            redundant_payload_bytes: 0,
            piece_hash_failures: 0,
            failed_piece_bytes: 0,
            last_hash_failure_contributors: 0,
        };
        state.append_piece_plans(plans)?;
        Ok(state)
    }

    pub fn append_piece_plans(&mut self, plans: Vec<PiecePlan>) -> Result<(), SwarmError> {
        for plan in plans {
            let plan_index = plan.index;
            if usize::try_from(plan.index).map_or(true, |index| index >= self.piece_count) {
                return Err(SwarmError::PieceOutOfRange {
                    piece: plan.index,
                    piece_count: self.piece_count,
                });
            }
            if self.pieces.contains_key(&plan.index) {
                return Err(SwarmError::DuplicatePiecePlan(plan.index));
            }
            let working_set_bytes = plan.blocks.iter().try_fold(0_usize, |total, block| {
                total
                    .checked_add(block.length as usize)
                    .ok_or(SwarmError::ArithmeticOverflow("piece working-set bytes"))
            })?;
            for block in &plan.blocks {
                if self
                    .blocks
                    .insert(
                        *block,
                        BlockState {
                            phase: BlockPhase::Missing,
                            attempts: VecDeque::new(),
                        },
                    )
                    .is_some()
                {
                    return Err(SwarmError::DuplicateBlock(*block));
                }
            }
            let block_count = plan.blocks.len();
            self.pieces.insert(
                plan.index,
                PieceState {
                    storage: PieceStorageJoin::new(plan.blocks.len()),
                    missing_blocks: plan.blocks.len(),
                    active_blocks: 0,
                    first_missing_block: 0,
                    blocks: plan.blocks,
                    working_set_bytes,
                },
            );
            self.incomplete_pieces.insert(
                usize::try_from(plan.index)
                    .map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?,
            );
            if self.inactive_rank_dirty {
                self.inactive_ranked_pieces.push(plan_index);
            } else {
                let position = self.inactive_ranked_pieces.partition_point(|candidate| {
                    !self
                        .picker
                        .precedes(plan_index as usize, *candidate as usize)
                });
                self.inactive_ranked_pieces.insert(position, plan_index);
            }
            self.picker
                .mark_planned(plan.index as usize)
                .map_err(SwarmError::Invariant)?;
            self.missing_blocks = self
                .missing_blocks
                .checked_add(block_count)
                .ok_or(SwarmError::ArithmeticOverflow("missing block count"))?;
        }
        Ok(())
    }

    pub fn retire_verified_piece(&mut self, piece: u32) -> Result<(), SwarmError> {
        let index =
            usize::try_from(piece).map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
        if self.incomplete_pieces.contains(&index) || self.active_pieces.contains(&index) {
            return Err(SwarmError::InvalidTransition(
                "piece cannot retire before verification completes",
            ));
        }
        let state = self
            .pieces
            .remove(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        for block in state.blocks {
            let block_state = self
                .blocks
                .remove(&block)
                .ok_or(SwarmError::UnknownBlock(block))?;
            if block_state.phase != BlockPhase::Verified {
                return Err(SwarmError::InvalidTransition(
                    "retired piece contains an unverified block",
                ));
            }
        }
        Ok(())
    }

    pub const fn config(&self) -> SwarmConfig {
        self.config
    }

    pub fn is_complete(&self) -> bool {
        self.picker.wanted_remaining() == 0
    }

    pub fn reserve_piece_for_planning(&mut self, maximum_planned_pieces: usize) -> Option<u32> {
        if self.pieces.len() >= maximum_planned_pieces {
            return None;
        }
        let connections = &self.connections;
        let ordinary = self.picker.reserve_best_matching(|piece| {
            connections
                .values()
                .any(|connection| connection.can_request_piece(piece))
        })?;
        let ordinary_availability = self.picker.availability(ordinary as usize)?;
        let suggested = self.connections.values().find_map(|connection| {
            connection.suggestions.iter().copied().find(|piece| {
                usize::try_from(*piece).is_ok_and(|piece| {
                    connection.can_request_piece(piece)
                        && self.picker.availability(piece) == Some(ordinary_availability)
                })
            })
        });
        let Some(suggested) = suggested else {
            return Some(ordinary);
        };
        self.picker
            .cancel_reserved(ordinary as usize)
            .expect("fresh picker reservation can be restored");
        if self.picker.reserve_specific(suggested as usize) {
            Some(suggested)
        } else {
            debug_assert!(self.picker.reserve_specific(ordinary as usize));
            Some(ordinary)
        }
    }

    pub fn cancel_piece_planning(&mut self, piece: u32) -> Result<(), SwarmError> {
        self.picker
            .cancel_reserved(piece as usize)
            .map_err(SwarmError::Invariant)
    }

    pub fn piece_availability(&self, piece: u32) -> Result<u32, SwarmError> {
        let index =
            usize::try_from(piece).map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
        self.picker
            .availability(index)
            .ok_or(SwarmError::PieceOutOfRange {
                piece,
                piece_count: self.piece_count,
            })
    }

    pub fn planned_piece_count(&self) -> usize {
        self.pieces.len()
    }

    pub fn planned_piece_bytes(&self) -> usize {
        self.pieces
            .values()
            .map(|piece| piece.working_set_bytes)
            .sum()
    }

    pub const fn outstanding_request_bytes(&self) -> usize {
        self.outstanding_request_bytes
    }

    pub const fn outstanding_request_high_water(&self) -> usize {
        self.outstanding_request_high_water
    }

    pub fn begin_dial(&mut self, id: PendingDialId) -> Result<(), SwarmError> {
        if self.pending_dials.contains(&id) {
            return Err(SwarmError::DuplicatePendingDial(id));
        }
        if self.pending_dials.len() == self.config.max_pending_dials {
            return Err(SwarmError::PendingDialCapacity);
        }
        self.pending_dials.insert(id);
        Ok(())
    }

    pub fn finish_dial(&mut self, id: PendingDialId) -> Result<(), SwarmError> {
        if !self.pending_dials.remove(&id) {
            return Err(SwarmError::UnknownPendingDial(id));
        }
        Ok(())
    }

    pub fn add_connection(&mut self, id: ConnectionId, now: Duration) -> Result<(), SwarmError> {
        if self.connections.contains_key(&id) {
            return Err(SwarmError::DuplicateConnection(id));
        }
        if self.connections.len() == self.config.max_established_connections {
            return Err(SwarmError::ConnectionCapacity);
        }
        self.connections.insert(
            id,
            ConnectionState {
                availability: PieceAvailability::empty(self.piece_count),
                wanted_piece_count: 0,
                choking: true,
                fast_extension: false,
                initial_availability_received: false,
                suggestions: VecDeque::new(),
                allowed_fast: BTreeSet::new(),
                connected_at: now,
                last_useful_at: None,
                active_request_count: 0,
                request_window: RequestWindow::new(
                    now,
                    self.config.initial_requests_per_connection,
                ),
            },
        );
        Ok(())
    }

    pub fn remove_connection(
        &mut self,
        id: ConnectionId,
        removal: ConnectionRemoval,
    ) -> Result<Vec<BlockKey>, SwarmError> {
        if !self.connections.contains_key(&id) {
            return Err(SwarmError::UnknownConnection(id));
        }
        let disposition = match removal {
            ConnectionRemoval::Disconnected | ConnectionRemoval::Replaced => {
                RequestDisposition::Disconnected
            }
            ConnectionRemoval::Cancelled => RequestDisposition::Cancelled,
        };
        let released = self.release_requests_for_connection(id, disposition)?;
        let removed = self
            .connections
            .remove(&id)
            .ok_or(SwarmError::UnknownConnection(id))?;
        if removed.active_request_count != 0 {
            return Err(SwarmError::Invariant(
                "removed connection retained active requests",
            ));
        }
        self.remove_availability_contribution(&removed.availability)?;
        self.picker.rebuild_after_bulk_update();
        self.inactive_rank_dirty = true;
        Ok(released)
    }

    pub fn set_bitfield(
        &mut self,
        id: ConnectionId,
        availability: Vec<bool>,
    ) -> Result<(), SwarmError> {
        if availability.len() != self.piece_count {
            return Err(SwarmError::InvalidAvailability {
                actual: availability.len(),
                expected: self.piece_count,
            });
        }
        self.mark_initial_availability(id)?;
        self.replace_connection_availability(id, PieceAvailability::from_flags(availability))
    }

    pub fn set_compact_bitfield(
        &mut self,
        id: ConnectionId,
        availability: Vec<u8>,
    ) -> Result<(), SwarmError> {
        let actual = availability.len().saturating_mul(8);
        let availability = PieceAvailability::from_bytes(availability, self.piece_count).ok_or(
            SwarmError::InvalidAvailability {
                actual,
                expected: self.piece_count,
            },
        )?;
        self.mark_initial_availability(id)?;
        self.replace_connection_availability(id, availability)
    }

    pub fn set_have_all(&mut self, id: ConnectionId) -> Result<(), SwarmError> {
        self.mark_initial_availability(id)?;
        self.replace_connection_availability(id, PieceAvailability::full(self.piece_count))
    }

    pub fn set_have_none(&mut self, id: ConnectionId) -> Result<(), SwarmError> {
        self.mark_initial_availability(id)?;
        self.replace_connection_availability(id, PieceAvailability::empty(self.piece_count))
    }

    pub fn peer_has(&mut self, id: ConnectionId, piece: u32) -> Result<(), SwarmError> {
        let piece_count = self.piece_count;
        let index = usize::try_from(piece)
            .map_err(|_| SwarmError::PieceOutOfRange { piece, piece_count })?;
        if index >= piece_count {
            return Err(SwarmError::PieceOutOfRange { piece, piece_count });
        }
        let wanted = self.picker.is_wanted(index);
        let became_seed = {
            let connection = self.connection_mut(id)?;
            if !connection.availability.set(index) {
                return Ok(());
            }
            if wanted {
                connection.wanted_piece_count =
                    connection.wanted_piece_count.checked_add(1).ok_or(
                        SwarmError::ArithmeticOverflow("connection wanted piece count"),
                    )?;
            }
            connection.availability.is_full()
        };
        self.picker
            .increment_piece(index)
            .map_err(SwarmError::Invariant)?;
        self.inactive_rank_dirty = true;
        if became_seed {
            let pieces = self
                .connections
                .get(&id)
                .ok_or(SwarmError::UnknownConnection(id))?
                .availability
                .set_pieces()
                .collect::<Vec<_>>();
            for piece in pieces {
                self.picker
                    .decrement_piece_without_repair(piece)
                    .map_err(SwarmError::Invariant)?;
            }
            self.picker
                .increment_seed_without_rebuild()
                .map_err(SwarmError::Invariant)?;
            self.picker.rebuild_after_bulk_update();
        }
        Ok(())
    }

    pub fn set_choking(
        &mut self,
        id: ConnectionId,
        choking: bool,
    ) -> Result<Vec<BlockKey>, SwarmError> {
        let changed = self.connection_mut(id)?.choking != choking;
        self.connection_mut(id)?.choking = choking;
        if changed {
            self.picker.note_eligibility_change();
        }
        if choking && !self.connection(id)?.fast_extension {
            self.release_requests_for_connection(id, RequestDisposition::Choked)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn set_fast_extension(
        &mut self,
        id: ConnectionId,
        enabled: bool,
    ) -> Result<(), SwarmError> {
        let connection = self.connection_mut(id)?;
        if connection.active_request_count != 0 {
            return Err(SwarmError::InvalidTransition(
                "Fast capability cannot change after requests begin",
            ));
        }
        connection.fast_extension = enabled;
        connection.initial_availability_received = false;
        connection.suggestions.clear();
        connection.allowed_fast.clear();
        Ok(())
    }

    pub fn observe_fast_message(
        &mut self,
        id: ConnectionId,
        message: &PeerMessage,
    ) -> Result<(), SwarmError> {
        let piece_count = self.piece_count;
        let connection = self.connection_mut(id)?;
        let fast_message = matches!(
            message,
            PeerMessage::SuggestPiece(_)
                | PeerMessage::HaveAll
                | PeerMessage::HaveNone
                | PeerMessage::RejectRequest(_)
                | PeerMessage::AllowedFast(_)
        );
        if !connection.fast_extension {
            return if fast_message {
                Err(SwarmError::InvalidTransition(
                    "Fast message arrived without bilateral negotiation",
                ))
            } else {
                Ok(())
            };
        }
        let initial = matches!(
            message,
            PeerMessage::Bitfield(_) | PeerMessage::HaveAll | PeerMessage::HaveNone
        );
        if !connection.initial_availability_received && !initial {
            let requires_availability = fast_message
                || matches!(
                    message,
                    PeerMessage::Have(_)
                        | PeerMessage::Request(_)
                        | PeerMessage::Cancel(_)
                        | PeerMessage::Piece { .. }
                );
            return if requires_availability {
                Err(SwarmError::InvalidTransition(
                    "Fast initial availability is missing",
                ))
            } else {
                Ok(())
            };
        }
        if connection.initial_availability_received && initial {
            return Err(SwarmError::InvalidTransition(
                "Fast initial availability was repeated",
            ));
        }
        let advisory = match message {
            PeerMessage::SuggestPiece(piece) => Some((*piece, true)),
            PeerMessage::AllowedFast(piece) => Some((*piece, false)),
            _ => None,
        };
        if let Some((piece, suggestion)) = advisory {
            if usize::try_from(piece).map_or(true, |piece| piece >= piece_count) {
                return Err(SwarmError::PieceOutOfRange { piece, piece_count });
            }
            if suggestion {
                if !connection.suggestions.contains(&piece)
                    && connection.suggestions.len() < MAX_FAST_ADVISORY_PIECES
                {
                    connection.suggestions.push_front(piece);
                }
            } else if connection.allowed_fast.len() < MAX_FAST_ADVISORY_PIECES {
                connection.allowed_fast.insert(piece);
            }
        }
        Ok(())
    }

    pub fn fast_peer_snapshot(&self, id: ConnectionId) -> Result<FastPeerSnapshot, SwarmError> {
        let connection = self.connection(id)?;
        Ok(FastPeerSnapshot {
            negotiated: connection.fast_extension,
            initial_availability_received: connection.initial_availability_received,
            suggestions: connection.suggestions.len(),
            allowed_fast: connection.allowed_fast.len(),
        })
    }

    pub fn reject_request(
        &mut self,
        connection: ConnectionId,
        block: BlockKey,
    ) -> Result<RejectDisposition, SwarmError> {
        if !self.connection(connection)?.fast_extension {
            return Ok(RejectDisposition::NeverRequested);
        }
        let Some(state) = self.blocks.get(&block) else {
            return Ok(RejectDisposition::NeverRequested);
        };
        let matching = state
            .attempts
            .iter()
            .filter(|attempt| attempt.connection == connection)
            .collect::<Vec<_>>();
        let Some(active) = matching
            .iter()
            .find(|attempt| attempt.disposition.is_active())
        else {
            return Ok(if matching.is_empty() {
                RejectDisposition::NeverRequested
            } else {
                RejectDisposition::Stale
            });
        };
        let attempt = active.id;
        self.terminate_requested(block, attempt, RequestDisposition::Rejected)?;
        let connection = self.connection_mut(connection)?;
        connection.allowed_fast.remove(&block.piece);
        connection.suggestions.retain(|piece| *piece != block.piece);
        Ok(RejectDisposition::Accepted { attempt })
    }

    pub fn schedule(&mut self, now: Duration) -> Result<Vec<RequestAssignment>, SwarmError> {
        for connection in self.connections.values_mut() {
            connection.request_window.refresh(now, self.config);
        }
        let mut assignments = Vec::new();
        loop {
            let mut progress = false;
            let mut ordered = self.ordered_connection_ids();
            ordered.retain(|connection| {
                self.connections.get(connection).is_some_and(|state| {
                    (!state.choking || state.has_choked_fast_work())
                        && state.active_request_count < state.request_window.target
                })
            });
            if ordered.is_empty() {
                break;
            }
            for connection in ordered.iter().copied() {
                let Some(block) = self.next_active_block_for_connection(connection)? else {
                    continue;
                };
                if !self.request_budget_allows(block)? {
                    continue;
                }
                let assignment = self.assign(connection, block, now, false)?;
                assignments.push(assignment);
                self.last_scheduled_connection = Some(connection);
                progress = true;
            }
            if progress {
                continue;
            }
            let Some((connection, block)) = self.next_inactive_assignment(&ordered)? else {
                break;
            };
            let assignment = self.assign(connection, block, now, false)?;
            assignments.push(assignment);
            self.last_scheduled_connection = Some(connection);
        }
        if self.missing_blocks == 0 {
            for connection in self.ordered_connection_ids() {
                if self.connection_request_count(connection) != 0 {
                    continue;
                }
                let Some(block) = self.next_endgame_block_for_connection(connection)? else {
                    continue;
                };
                let length = usize::try_from(block.length)
                    .map_err(|_| SwarmError::ArithmeticOverflow("block length"))?;
                if self
                    .outstanding_request_bytes
                    .checked_add(length)
                    .is_none_or(|reserved| reserved > self.config.max_outstanding_request_bytes)
                {
                    continue;
                }
                assignments.push(self.assign(connection, block, now, true)?);
                self.last_scheduled_connection = Some(connection);
            }
        }
        Ok(assignments)
    }

    pub fn expire_requests(&mut self, now: Duration) -> Result<Vec<ExpiredRequest>, SwarmError> {
        let mut expired = Vec::new();
        let active = self
            .active_attempts
            .values()
            .copied()
            .map(|attempt| (attempt.block, attempt))
            .collect::<Vec<_>>();

        let mut stalled_connections = self
            .connections
            .iter()
            .filter_map(|(id, connection)| {
                let oldest = active
                    .iter()
                    .filter(|(_, attempt)| attempt.connection == *id)
                    .map(|(_, attempt)| attempt.issued_at)
                    .min()?;
                (now >= connection
                    .request_window
                    .stall_deadline(oldest, self.config))
                .then_some(*id)
            })
            .collect::<BTreeSet<_>>();

        for (block, attempt) in active {
            let expired_by_connection = stalled_connections.contains(&attempt.connection);
            let expired_by_request =
                now.saturating_sub(attempt.issued_at) >= self.config.request_timeout;
            if !expired_by_connection && !expired_by_request {
                continue;
            }
            if expired_by_request {
                stalled_connections.insert(attempt.connection);
            }
            self.terminate_requested(block, attempt.id, RequestDisposition::Expired)?;
            expired.push(ExpiredRequest {
                attempt: attempt.id,
                connection: attempt.connection,
                block,
            });
        }
        for connection in stalled_connections {
            if let Some(connection) = self.connections.get_mut(&connection) {
                connection.request_window.stalled(now);
            }
        }
        Ok(expired)
    }

    pub fn defer_peer_deadlines(&mut self, delay: Duration) {
        if delay.is_zero() {
            return;
        }
        for attempt in self.active_attempts.values_mut() {
            attempt.issued_at = attempt.issued_at.saturating_add(delay);
            if let Some(retained) = self.blocks.get_mut(&attempt.block).and_then(|block| {
                block
                    .attempts
                    .iter_mut()
                    .find(|retained| retained.id == attempt.id)
            }) {
                retained.issued_at = attempt.issued_at;
            }
        }
        for connection in self.connections.values_mut() {
            connection.connected_at = connection.connected_at.saturating_add(delay);
            connection.last_useful_at = connection
                .last_useful_at
                .map(|instant| instant.saturating_add(delay));
            connection.request_window.sample_started_at = connection
                .request_window
                .sample_started_at
                .saturating_add(delay);
            connection.request_window.last_payload_at = connection
                .request_window
                .last_payload_at
                .map(|instant| instant.saturating_add(delay));
        }
    }

    pub fn receive_block(
        &mut self,
        connection: ConnectionId,
        block: BlockKey,
        now: Duration,
    ) -> Result<ReceiveDisposition, SwarmError> {
        if !self.connections.contains_key(&connection) {
            return Err(SwarmError::UnknownConnection(connection));
        }
        let Some(state) = self.blocks.get(&block) else {
            return Ok(ReceiveDisposition::Unsolicited);
        };
        let evidence = state
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.connection == connection)
            .copied();
        if !matches!(state.phase, BlockPhase::Requested) {
            if evidence.is_some() {
                self.redundant_payload_bytes = self
                    .redundant_payload_bytes
                    .saturating_add(block.length as usize);
                return Ok(ReceiveDisposition::Redundant);
            }
            return Ok(ReceiveDisposition::Unsolicited);
        }
        let Some(evidence) = evidence else {
            return Ok(ReceiveDisposition::Unsolicited);
        };
        let active_attempts = state
            .attempts
            .iter()
            .filter(|attempt| attempt.disposition.is_active())
            .copied()
            .collect::<Vec<_>>();
        if active_attempts.is_empty() {
            return Err(SwarmError::Invariant(
                "requested block has no active request attempt",
            ));
        }
        let evidence_is_active = evidence.disposition.is_active();
        let late = !evidence_is_active;
        let request_issued_at = evidence_is_active.then_some(evidence.issued_at);
        let cancellations = active_attempts
            .iter()
            .filter(|attempt| attempt.id != evidence.id)
            .map(|attempt| RequestCancellation {
                attempt: attempt.id,
                connection: attempt.connection,
                block,
            })
            .collect::<Vec<_>>();
        {
            let state = self
                .blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?;
            for attempt in &mut state.attempts {
                if attempt.id == evidence.id {
                    attempt.disposition = RequestDisposition::PayloadReceived;
                } else if attempt.disposition.is_active() {
                    attempt.disposition = RequestDisposition::Superseded;
                }
            }
            state.phase = BlockPhase::Writing {
                source: connection,
                evidence: evidence.id,
            };
            trim_terminal_attempts(
                state,
                self.config.max_terminal_attempts_per_block,
                Some(evidence.id),
            )?;
        }
        self.requested_blocks = self
            .requested_blocks
            .checked_sub(1)
            .ok_or(SwarmError::Invariant("requested block count underflow"))?;
        self.writing_blocks = self
            .writing_blocks
            .checked_add(1)
            .ok_or(SwarmError::ArithmeticOverflow("writing block count"))?;
        self.note_unverified_contribution(connection)?;
        for attempt in &active_attempts {
            if self.active_attempts.remove(&attempt.id).is_none() {
                return Err(SwarmError::Invariant(
                    "received block attempt is absent from the active index",
                ));
            }
            self.release_connection_request(attempt.connection)?;
            self.release_request(block.length)?;
        }
        self.cancelled_request_attempts = self
            .cancelled_request_attempts
            .saturating_add(cancellations.len());
        let config = self.config;
        self.connection_mut(connection)?
            .request_window
            .accepted_payload(now, block.length as usize, request_issued_at, config);
        Ok(ReceiveDisposition::Accept {
            evidence: evidence.id,
            cancellations,
            late,
        })
    }

    pub fn piece_generation(&self, piece: u32) -> Result<PieceGeneration, SwarmError> {
        self.pieces
            .get(&piece)
            .map(|piece| piece.storage.generation)
            .ok_or(SwarmError::UnknownPiece(piece))
    }

    pub fn finish_write(
        &mut self,
        block: BlockKey,
        accepted: bool,
        now: Duration,
    ) -> Result<(), SwarmError> {
        let generation = self.piece_generation(block.piece)?;
        self.finish_write_for_generation(block, generation, accepted, now)
    }

    pub fn finish_write_for_generation(
        &mut self,
        block: BlockKey,
        generation: PieceGeneration,
        accepted: bool,
        now: Duration,
    ) -> Result<(), SwarmError> {
        self.pieces
            .get(&block.piece)
            .ok_or(SwarmError::UnknownPiece(block.piece))?
            .storage
            .validate_generation(generation)?;
        let (source, evidence) = match self
            .blocks
            .get(&block)
            .ok_or(SwarmError::UnknownBlock(block))?
            .phase
        {
            BlockPhase::Writing { source, evidence } => (source, evidence),
            _ => return Err(SwarmError::InvalidTransition("block is not being written")),
        };
        if !self
            .blocks
            .get(&block)
            .is_some_and(|state| state.attempts.iter().any(|attempt| attempt.id == evidence))
        {
            return Err(SwarmError::Invariant("write evidence attempt is missing"));
        }
        let state = self
            .blocks
            .get_mut(&block)
            .ok_or(SwarmError::UnknownBlock(block))?;
        state.phase = if accepted {
            BlockPhase::Received { source, evidence }
        } else {
            BlockPhase::Missing
        };
        trim_terminal_attempts(
            state,
            self.config.max_terminal_attempts_per_block,
            accepted.then_some(evidence),
        )?;
        self.writing_blocks = self
            .writing_blocks
            .checked_sub(1)
            .ok_or(SwarmError::Invariant("writing block count underflow"))?;
        if accepted {
            self.received_blocks = self
                .received_blocks
                .checked_add(1)
                .ok_or(SwarmError::ArithmeticOverflow("received block count"))?;
        } else {
            self.missing_blocks = self
                .missing_blocks
                .checked_add(1)
                .ok_or(SwarmError::ArithmeticOverflow("missing block count"))?;
            self.note_piece_block_became_missing(block)?;
            self.release_unverified_contribution(source)?;
            self.deactivate_piece_if_idle(block.piece)?;
            self.refresh_requestable_piece(block.piece)?;
        }
        if accepted && let Some(connection) = self.connections.get_mut(&source) {
            connection.last_useful_at = Some(now);
        }
        self.pieces
            .get_mut(&block.piece)
            .ok_or(SwarmError::UnknownPiece(block.piece))?
            .storage
            .write_completed(generation, accepted)?;
        Ok(())
    }

    pub fn note_useful_payload(
        &mut self,
        connection: ConnectionId,
        now: Duration,
    ) -> Result<(), SwarmError> {
        self.connection_mut(connection)?.last_useful_at = Some(now);
        Ok(())
    }

    pub fn piece_ready(&self, piece: u32) -> Result<bool, SwarmError> {
        let generation = self.piece_generation(piece)?;
        self.piece_ready_for_generation(piece, generation)
    }

    pub fn piece_ready_for_generation(
        &self,
        piece: u32,
        generation: PieceGeneration,
    ) -> Result<bool, SwarmError> {
        let piece = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        piece.storage.hash_eligible(generation)
    }

    pub fn begin_piece_hash(
        &mut self,
        piece: u32,
        generation: PieceGeneration,
    ) -> Result<(), SwarmError> {
        let piece = self
            .pieces
            .get_mut(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        if !piece.storage.hash_eligible(generation)? {
            return Err(SwarmError::InvalidTransition(
                "piece cannot hash before every write succeeds",
            ));
        }
        piece.storage.begin_hash(generation)
    }

    pub fn finish_piece_hash(
        &mut self,
        piece: u32,
        generation: PieceGeneration,
        passed: bool,
    ) -> Result<(), SwarmError> {
        self.pieces
            .get_mut(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .storage
            .hash_completed(generation, passed)?;
        Ok(())
    }

    pub fn mark_piece_verified(&mut self, piece: u32) -> Result<Vec<ConnectionId>, SwarmError> {
        let generation = self.piece_generation(piece)?;
        self.begin_piece_hash(piece, generation)?;
        self.finish_piece_hash(piece, generation, true)?;
        self.mark_piece_verified_for_generation(piece, generation)
    }

    pub fn mark_piece_verified_for_generation(
        &mut self,
        piece: u32,
        generation: PieceGeneration,
    ) -> Result<Vec<ConnectionId>, SwarmError> {
        let blocks = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .blocks
            .clone();
        let storage = &self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .storage;
        storage.validate_generation(generation)?;
        if storage.outcome() != PieceStorageOutcome::Verified {
            return Err(SwarmError::InvalidTransition(
                "piece cannot verify before its write/hash join passes",
            ));
        }
        if !blocks.iter().all(|block| {
            self.blocks
                .get(block)
                .is_some_and(|state| matches!(state.phase, BlockPhase::Received { .. }))
        }) {
            return Err(SwarmError::InvalidTransition(
                "piece cannot verify before every block is newly stored",
            ));
        }
        let contributor_sources = self.piece_contributor_sources(&blocks)?;
        let contributors = contributor_sources
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let block_count = blocks.len();
        let piece_state = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        if piece_state.missing_blocks != 0 || piece_state.active_blocks != block_count {
            return Err(SwarmError::Invariant(
                "verified piece block counters do not match received blocks",
            ));
        }
        for block in blocks {
            self.blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?
                .phase = BlockPhase::Verified;
        }
        self.received_blocks = self
            .received_blocks
            .checked_sub(block_count)
            .ok_or(SwarmError::Invariant("received block count underflow"))?;
        self.verified_blocks = self
            .verified_blocks
            .checked_add(block_count)
            .ok_or(SwarmError::ArithmeticOverflow("verified block count"))?;
        let piece_state = self
            .pieces
            .get_mut(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        piece_state.active_blocks = 0;
        piece_state.first_missing_block = piece_state.blocks.len();
        for source in contributor_sources {
            self.release_unverified_contribution(source)?;
        }
        let piece_index =
            usize::try_from(piece).map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
        if !self.incomplete_pieces.remove(&piece_index) {
            return Err(SwarmError::Invariant(
                "verified piece is absent from the incomplete index",
            ));
        }
        self.picker
            .mark_completed(piece_index)
            .map_err(SwarmError::Invariant)?;
        let ranked_position = self
            .inactive_ranked_pieces
            .iter()
            .position(|&candidate| candidate == piece)
            .ok_or(SwarmError::Invariant(
                "verified piece is absent from the inactive rank",
            ))?;
        self.inactive_ranked_pieces.remove(ranked_position);
        for connection in self.connections.values_mut() {
            if connection.availability.contains(piece_index) {
                connection.wanted_piece_count = connection
                    .wanted_piece_count
                    .checked_sub(1)
                    .ok_or(SwarmError::Invariant(
                        "connection wanted piece count underflow",
                    ))?;
            }
        }
        self.deactivate_piece(piece)?;
        Ok(contributors)
    }

    pub fn mark_piece_hash_failed(&mut self, piece: u32) -> Result<PieceHashFailure, SwarmError> {
        let generation = self.piece_generation(piece)?;
        self.begin_piece_hash(piece, generation)?;
        self.finish_piece_hash(piece, generation, false)?;
        self.mark_piece_hash_failed_for_generation(piece, generation)
    }

    pub fn mark_piece_hash_failed_for_generation(
        &mut self,
        piece: u32,
        generation: PieceGeneration,
    ) -> Result<PieceHashFailure, SwarmError> {
        let blocks = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .blocks
            .clone();
        let storage = &self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .storage;
        storage.validate_generation(generation)?;
        if storage.outcome() != PieceStorageOutcome::Failed {
            return Err(SwarmError::InvalidTransition(
                "piece cannot reset before its write/hash join fails",
            ));
        }
        if !blocks.iter().all(|block| {
            self.blocks
                .get(block)
                .is_some_and(|state| matches!(state.phase, BlockPhase::Received { .. }))
        }) {
            return Err(SwarmError::InvalidTransition(
                "piece cannot fail its hash before every block is stored",
            ));
        }
        let contributor_sources = self.piece_contributor_sources(&blocks)?;
        let contributors = contributor_sources
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let failed_bytes = blocks.iter().try_fold(0_usize, |total, block| {
            total
                .checked_add(block.length as usize)
                .ok_or(SwarmError::ArithmeticOverflow("failed piece bytes"))
        })?;
        let block_count = blocks.len();
        let piece_state = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        if piece_state.missing_blocks != 0 || piece_state.active_blocks != block_count {
            return Err(SwarmError::Invariant(
                "failed piece block counters do not match received blocks",
            ));
        }
        for block in blocks {
            self.blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?
                .phase = BlockPhase::Missing;
        }
        self.received_blocks = self
            .received_blocks
            .checked_sub(block_count)
            .ok_or(SwarmError::Invariant("received block count underflow"))?;
        self.missing_blocks = self
            .missing_blocks
            .checked_add(block_count)
            .ok_or(SwarmError::ArithmeticOverflow("missing block count"))?;
        let piece_state = self
            .pieces
            .get_mut(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        piece_state.missing_blocks = block_count;
        piece_state.active_blocks = 0;
        piece_state.first_missing_block = 0;
        for source in contributor_sources {
            self.release_unverified_contribution(source)?;
        }
        self.deactivate_piece(piece)?;
        self.pieces
            .get_mut(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .storage
            .reset()?;
        self.piece_hash_failures = self.piece_hash_failures.saturating_add(1);
        self.failed_piece_bytes = self.failed_piece_bytes.saturating_add(failed_bytes);
        self.last_hash_failure_contributors = contributors.len();
        Ok(PieceHashFailure {
            piece,
            contributors,
            failed_bytes,
        })
    }

    fn piece_contributor_sources(
        &self,
        blocks: &[BlockKey],
    ) -> Result<Vec<ConnectionId>, SwarmError> {
        blocks
            .iter()
            .map(|block| {
                let state = self
                    .blocks
                    .get(block)
                    .ok_or(SwarmError::UnknownBlock(*block))?;
                match state.phase {
                    BlockPhase::Received { source, evidence } => {
                        if !state.attempts.iter().any(|attempt| {
                            attempt.id == evidence
                                && attempt.connection == source
                                && attempt.disposition == RequestDisposition::PayloadReceived
                        }) {
                            return Err(SwarmError::Invariant(
                                "stored block contributor evidence is missing",
                            ));
                        }
                        Ok(source)
                    }
                    BlockPhase::Missing
                    | BlockPhase::Requested
                    | BlockPhase::Writing { .. }
                    | BlockPhase::Verified => Err(SwarmError::InvalidTransition(
                        "piece contributor requested before storage completion",
                    )),
                }
            })
            .collect()
    }

    pub(crate) fn unverified_contributors(&self) -> BTreeSet<ConnectionId> {
        self.unverified_contributor_blocks.keys().copied().collect()
    }

    pub fn cancel_all(&mut self) -> Result<(), SwarmError> {
        let requested = self
            .active_attempts
            .values()
            .map(|attempt| (attempt.block, attempt.id))
            .collect::<Vec<_>>();
        for (block, attempt) in requested {
            self.terminate_requested(block, attempt, RequestDisposition::Cancelled)?;
        }
        let writing = self
            .blocks
            .iter()
            .filter_map(|(block, state)| match state.phase {
                BlockPhase::Writing { source, .. } => Some((*block, source)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (block, source) in writing {
            self.blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?
                .phase = BlockPhase::Missing;
            self.writing_blocks = self
                .writing_blocks
                .checked_sub(1)
                .ok_or(SwarmError::Invariant("writing block count underflow"))?;
            self.missing_blocks = self
                .missing_blocks
                .checked_add(1)
                .ok_or(SwarmError::ArithmeticOverflow("missing block count"))?;
            self.note_piece_block_became_missing(block)?;
            self.release_unverified_contribution(source)?;
        }
        self.active_pieces.clear();
        self.requestable_active_pieces.clear();
        self.requestable_active_block_keys.clear();
        self.requestable_active_blocks = 0;
        self.active_piece_bytes = 0;
        self.pending_dials.clear();
        Ok(())
    }

    pub fn replacement_candidate(&self, now: Duration) -> Option<ConnectionId> {
        if self.connections.len() < self.config.max_established_connections {
            return None;
        }
        self.connections
            .iter()
            .filter(|(id, connection)| {
                now.saturating_sub(connection.last_useful_at.unwrap_or(connection.connected_at))
                    >= self.config.unproductive_grace
                    && !self.has_unique_wanted_piece(**id)
            })
            .filter_map(|(id, connection)| {
                let priority = if connection.wanted_piece_count == 0 {
                    0_u8
                } else if connection.choking {
                    1
                } else if self.connection_request_count(*id) == 0 {
                    2
                } else {
                    return None;
                };
                Some((
                    priority,
                    connection.last_useful_at,
                    connection.connected_at,
                    *id,
                ))
            })
            .min()
            .map(|(_, _, _, id)| id)
    }

    pub fn block_status(&self, block: BlockKey) -> Result<BlockStatus, SwarmError> {
        self.blocks
            .get(&block)
            .map(|block| block.phase.status())
            .ok_or(SwarmError::UnknownBlock(block))
    }

    pub fn attempts_for(&self, block: BlockKey) -> Result<Vec<RequestAttempt>, SwarmError> {
        self.blocks
            .get(&block)
            .map(|block| block.attempts.iter().copied().collect())
            .ok_or(SwarmError::UnknownBlock(block))
    }

    pub fn snapshot(&self, now: Duration) -> SwarmSnapshot {
        let mut oldest_issued = None;
        let mut next_expiry = None;
        let mut oldest_by_connection = BTreeMap::new();
        for attempt in self.active_attempts.values() {
            oldest_issued = Some(oldest_issued.map_or(attempt.issued_at, |oldest: Duration| {
                oldest.min(attempt.issued_at)
            }));
            oldest_by_connection
                .entry(attempt.connection)
                .and_modify(|oldest: &mut Duration| {
                    *oldest = (*oldest).min(attempt.issued_at);
                })
                .or_insert(attempt.issued_at);
            let deadline = attempt
                .issued_at
                .checked_add(self.config.request_timeout)
                .unwrap_or(Duration::MAX);
            next_expiry = Some(next_expiry.map_or(deadline, |next: Duration| next.min(deadline)));
        }
        let mut request_timeout_min = None;
        let mut request_timeout_max = None;
        for (id, connection) in &self.connections {
            let timeout = connection.request_window.request_timeout(self.config);
            request_timeout_min =
                Some(request_timeout_min.map_or(timeout, |current: Duration| current.min(timeout)));
            request_timeout_max =
                Some(request_timeout_max.map_or(timeout, |current: Duration| current.max(timeout)));
            if let Some(&oldest) = oldest_by_connection.get(id) {
                let deadline = connection
                    .request_window
                    .stall_deadline(oldest, self.config);
                next_expiry =
                    Some(next_expiry.map_or(deadline, |next: Duration| next.min(deadline)));
            }
        }
        let picker_counters = self.picker.counters();
        SwarmSnapshot {
            pending_dials: self.pending_dials.len(),
            connected_peers: self.connections.len(),
            unchoked_peers: self
                .connections
                .values()
                .filter(|connection| !connection.choking)
                .count(),
            missing_blocks: self.missing_blocks,
            requested_blocks: self.requested_blocks,
            active_request_attempts: self.active_attempts.len(),
            active_duplicate_attempts: self
                .active_attempts
                .len()
                .saturating_sub(self.requested_blocks),
            writing_blocks: self.writing_blocks,
            received_blocks: self.received_blocks,
            verified_blocks: self.verified_blocks,
            active_piece_count: self.active_pieces.len(),
            active_piece_bytes: self.active_piece_bytes,
            max_active_pieces: self.config.max_active_pieces,
            piece_activation_policy: self.picker.policy(),
            availability_seed_count: self.picker.seed_count() as usize,
            picker_retained_bytes: self.picker.retained_bytes(),
            picker_rank_comparisons: picker_counters.rank_comparisons,
            picker_bulk_rebuilds: picker_counters.bulk_rebuilds,
            picker_candidate_inspections: picker_counters.candidate_inspections,
            active_piece_visits: self.active_piece_visits,
            inactive_planned_piece_visits: self.inactive_planned_piece_visits,
            last_activated_piece: self.last_activated_piece,
            last_activated_availability: self.last_activated_availability,
            outstanding_request_bytes: self.outstanding_request_bytes,
            outstanding_request_high_water: self.outstanding_request_high_water,
            request_target_total: self
                .connections
                .values()
                .map(|connection| connection.request_window.target)
                .sum(),
            request_target_max: self
                .connections
                .values()
                .map(|connection| connection.request_window.target)
                .max()
                .unwrap_or(0),
            slow_start_peers: self
                .connections
                .values()
                .filter(|connection| {
                    connection.request_window.phase == RequestWindowPhase::SlowStart
                })
                .count(),
            stalled_peers: self
                .connections
                .values()
                .filter(|connection| connection.request_window.phase == RequestWindowPhase::Stalled)
                .count(),
            useful_payload_bytes: self
                .connections
                .values()
                .map(|connection| connection.request_window.useful_payload_bytes)
                .sum(),
            observed_payload_rate: self
                .connections
                .values()
                .map(|connection| connection.request_window.observed_payload_rate)
                .sum(),
            endgame_assignments: self.endgame_assignments,
            cancelled_request_attempts: self.cancelled_request_attempts,
            redundant_payload_bytes: self.redundant_payload_bytes,
            piece_hash_failures: self.piece_hash_failures,
            failed_piece_bytes: self.failed_piece_bytes,
            last_hash_failure_contributors: self.last_hash_failure_contributors,
            request_timeout_min,
            request_timeout_max,
            oldest_request_age: oldest_issued.map(|issued| now.saturating_sub(issued)),
            next_request_expiry: next_expiry,
            next_replacement_at: self.next_replacement_at(),
            no_request_reason: self.no_request_reason(),
        }
    }

    pub(crate) fn connection_activity(&self, now: Duration) -> Vec<ConnectionActivitySnapshot> {
        let mut requests = BTreeMap::<ConnectionId, (usize, usize, Option<Duration>)>::new();
        for attempt in self.active_attempts.values() {
            let entry = requests.entry(attempt.connection).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(attempt.block.length as usize);
            entry.2 = Some(
                entry
                    .2
                    .map_or(attempt.issued_at, |oldest| oldest.min(attempt.issued_at)),
            );
        }
        self.connections
            .iter()
            .map(|(id, connection)| {
                let (pending_requests, queued_payload_bytes, oldest_request_age) = requests
                    .get(id)
                    .map(|(count, bytes, oldest)| {
                        (
                            *count,
                            *bytes,
                            oldest.map(|value| now.saturating_sub(value)),
                        )
                    })
                    .unwrap_or_default();
                let window_phase = match connection.request_window.phase {
                    RequestWindowPhase::SlowStart => ConnectionWindowPhaseSnapshot::SlowStart,
                    RequestWindowPhase::Steady => ConnectionWindowPhaseSnapshot::Steady,
                    RequestWindowPhase::Stalled => ConnectionWindowPhaseSnapshot::Stalled,
                };
                ConnectionActivitySnapshot {
                    id: *id,
                    choking: connection.choking,
                    wanted_piece_count: connection.wanted_piece_count,
                    pending_requests,
                    target_requests: connection.request_window.target,
                    queued_payload_bytes,
                    window_phase,
                    useful_payload_bytes: connection.request_window.useful_payload_bytes,
                    observed_payload_rate: connection.request_window.observed_payload_rate,
                    connected_age: now.saturating_sub(connection.connected_at),
                    last_useful_age: connection
                        .last_useful_at
                        .map(|value| now.saturating_sub(value)),
                    last_payload_age: connection
                        .request_window
                        .last_payload_at
                        .map(|value| now.saturating_sub(value)),
                    request_timeout: connection.request_window.request_timeout(self.config),
                    oldest_request_age,
                }
            })
            .collect()
    }

    fn next_replacement_at(&self) -> Option<Duration> {
        if self.connections.len() < self.config.max_established_connections {
            return None;
        }
        self.connections
            .iter()
            .filter(|(id, _)| !self.has_unique_wanted_piece(**id))
            .filter(|(id, connection)| {
                connection.wanted_piece_count == 0
                    || connection.choking
                    || self.connection_request_count(**id) == 0
            })
            .filter_map(|(_, connection)| {
                connection
                    .last_useful_at
                    .unwrap_or(connection.connected_at)
                    .checked_add(self.config.unproductive_grace)
            })
            .min()
    }

    fn connection_mut(&mut self, id: ConnectionId) -> Result<&mut ConnectionState, SwarmError> {
        self.connections
            .get_mut(&id)
            .ok_or(SwarmError::UnknownConnection(id))
    }

    fn connection(&self, id: ConnectionId) -> Result<&ConnectionState, SwarmError> {
        self.connections
            .get(&id)
            .ok_or(SwarmError::UnknownConnection(id))
    }

    fn mark_initial_availability(&mut self, id: ConnectionId) -> Result<(), SwarmError> {
        let connection = self.connection_mut(id)?;
        if !connection.fast_extension {
            return Ok(());
        }
        if connection.initial_availability_received {
            return Err(SwarmError::InvalidTransition(
                "Fast initial availability was repeated",
            ));
        }
        connection.initial_availability_received = true;
        Ok(())
    }

    fn replace_connection_availability(
        &mut self,
        id: ConnectionId,
        availability: PieceAvailability,
    ) -> Result<(), SwarmError> {
        let wanted_piece_count = availability
            .set_pieces()
            .filter(|piece| self.picker.is_wanted(*piece))
            .count();
        let previous = {
            let connection = self.connection_mut(id)?;
            connection.wanted_piece_count = wanted_piece_count;
            std::mem::replace(&mut connection.availability, availability)
        };
        self.remove_availability_contribution(&previous)?;
        let current = self
            .connections
            .get(&id)
            .ok_or(SwarmError::UnknownConnection(id))?;
        if current.availability.is_full() {
            self.picker
                .increment_seed_without_rebuild()
                .map_err(SwarmError::Invariant)?;
        } else {
            for piece in current.availability.set_pieces() {
                self.picker
                    .increment_piece_without_repair(piece)
                    .map_err(SwarmError::Invariant)?;
            }
        }
        self.picker.rebuild_after_bulk_update();
        self.inactive_rank_dirty = true;
        Ok(())
    }

    fn remove_availability_contribution(
        &mut self,
        availability: &PieceAvailability,
    ) -> Result<(), SwarmError> {
        if availability.is_full() {
            self.picker
                .decrement_seed_without_rebuild()
                .map_err(SwarmError::Invariant)?;
        } else {
            for piece in availability.set_pieces() {
                self.picker
                    .decrement_piece_without_repair(piece)
                    .map_err(SwarmError::Invariant)?;
            }
        }
        Ok(())
    }

    fn ordered_connection_ids(&self) -> Vec<ConnectionId> {
        let mut ids = self.connections.keys().copied().collect::<Vec<_>>();
        if let Some(last) = self.last_scheduled_connection
            && let Some(index) = ids.iter().position(|id| *id == last)
        {
            let next = (index + 1) % ids.len();
            ids.rotate_left(next);
        }
        ids
    }

    fn next_active_block_for_connection(
        &mut self,
        connection: ConnectionId,
    ) -> Result<Option<BlockKey>, SwarmError> {
        let connection = self
            .connections
            .get(&connection)
            .ok_or(SwarmError::UnknownConnection(connection))?;
        for (&piece, &block) in &self.requestable_active_block_keys {
            self.active_piece_visits = self.active_piece_visits.saturating_add(1);
            if connection.can_request_piece(piece) {
                return Ok(Some(block));
            }
        }
        Ok(None)
    }

    fn next_inactive_assignment(
        &mut self,
        ordered_connections: &[ConnectionId],
    ) -> Result<Option<(ConnectionId, BlockKey)>, SwarmError> {
        if self.outstanding_request_bytes >= self.config.max_outstanding_request_bytes {
            return Ok(None);
        }
        if self.inactive_rank_dirty {
            let picker = &self.picker;
            self.inactive_ranked_pieces.sort_unstable_by(|lhs, rhs| {
                if picker.precedes(*lhs as usize, *rhs as usize) {
                    std::cmp::Ordering::Less
                } else if picker.precedes(*rhs as usize, *lhs as usize) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            self.inactive_rank_dirty = false;
        }
        for &piece_u32 in &self.inactive_ranked_pieces {
            self.inactive_planned_piece_visits =
                self.inactive_planned_piece_visits.saturating_add(1);
            let piece = usize::try_from(piece_u32)
                .map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
            if self.active_pieces.contains(&piece) || !self.can_activate_piece(piece) {
                continue;
            }
            let Some(block) = self.first_missing_block(piece_u32) else {
                continue;
            };
            if !self.request_budget_allows(block)? {
                continue;
            }
            let Some(connection) = ordered_connections.iter().copied().find(|connection| {
                self.connections
                    .get(connection)
                    .is_some_and(|state| state.can_request_piece(piece))
            }) else {
                continue;
            };
            return Ok(Some((connection, block)));
        }
        Ok(None)
    }

    fn request_budget_allows(&self, block: BlockKey) -> Result<bool, SwarmError> {
        let length = usize::try_from(block.length)
            .map_err(|_| SwarmError::ArithmeticOverflow("block length"))?;
        Ok(self
            .outstanding_request_bytes
            .checked_add(length)
            .is_some_and(|reserved| reserved <= self.config.max_outstanding_request_bytes))
    }

    fn next_endgame_block_for_connection(
        &self,
        connection: ConnectionId,
    ) -> Result<Option<BlockKey>, SwarmError> {
        let connection_state = self
            .connections
            .get(&connection)
            .ok_or(SwarmError::UnknownConnection(connection))?;
        Ok(self.blocks.iter().find_map(|(block, block_state)| {
            let piece = usize::try_from(block.piece).ok()?;
            (matches!(block_state.phase, BlockPhase::Requested)
                && block_state
                    .attempts
                    .iter()
                    .any(|attempt| attempt.disposition.is_active())
                && !block_state.attempts.iter().any(|attempt| {
                    attempt.disposition.is_active() && attempt.connection == connection
                })
                && connection_state.can_request_piece(piece))
            .then_some(*block)
        }))
    }

    fn first_missing_block(&self, piece: u32) -> Option<BlockKey> {
        let piece = self.pieces.get(&piece)?;
        piece.blocks.get(piece.first_missing_block).copied()
    }

    fn piece_working_set_bytes(&self, piece: usize) -> usize {
        let Ok(piece) = u32::try_from(piece) else {
            return usize::MAX;
        };
        self.pieces
            .get(&piece)
            .map_or(0, |state| state.working_set_bytes)
    }

    fn can_activate_piece(&self, piece: usize) -> bool {
        if self.active_pieces.is_empty() {
            return true;
        }
        if self.active_pieces.len() >= self.config.max_active_pieces {
            return false;
        }
        if u32::try_from(piece)
            .ok()
            .and_then(|piece| self.pieces.get(&piece))
            .is_none()
        {
            return false;
        }
        let peer_pressure = self.connections.len().saturating_mul(3) / 2;
        if self.requestable_active_pieces.len() > peer_pressure
            || self.requestable_active_blocks > 2_048
        {
            return false;
        }
        self.active_piece_bytes
            .checked_add(self.piece_working_set_bytes(piece))
            .is_some_and(|bytes| bytes <= self.config.max_active_piece_bytes)
    }

    fn assign(
        &mut self,
        connection: ConnectionId,
        block: BlockKey,
        now: Duration,
        endgame: bool,
    ) -> Result<RequestAssignment, SwarmError> {
        let attempt_id = RequestAttemptId(self.next_attempt_id);
        self.next_attempt_id = self
            .next_attempt_id
            .checked_add(1)
            .ok_or(SwarmError::IdentifierOverflow("request attempt"))?;
        let state = self
            .blocks
            .get_mut(&block)
            .ok_or(SwarmError::UnknownBlock(block))?;
        let valid_phase = if endgame {
            matches!(state.phase, BlockPhase::Requested)
                && state
                    .attempts
                    .iter()
                    .any(|attempt| attempt.disposition.is_active())
                && !state.attempts.iter().any(|attempt| {
                    attempt.disposition.is_active() && attempt.connection == connection
                })
        } else {
            matches!(state.phase, BlockPhase::Missing)
        };
        if !valid_phase {
            return Err(SwarmError::InvalidTransition(if endgame {
                "block cannot accept this endgame request"
            } else {
                "block is not missing"
            }));
        }
        trim_terminal_attempts(
            state,
            self.config
                .max_terminal_attempts_per_block
                .saturating_sub(1),
            None,
        )?;
        state.attempts.push_back(RequestAttempt {
            id: attempt_id,
            block,
            connection,
            issued_at: now,
            disposition: RequestDisposition::Requested,
        });
        state.phase = BlockPhase::Requested;
        if !endgame {
            self.missing_blocks = self
                .missing_blocks
                .checked_sub(1)
                .ok_or(SwarmError::Invariant("missing block count underflow"))?;
            self.requested_blocks = self
                .requested_blocks
                .checked_add(1)
                .ok_or(SwarmError::ArithmeticOverflow("requested block count"))?;
            self.note_piece_block_assigned(block)?;
            self.activate_piece(block.piece)?;
        }
        let attempt = RequestAttempt {
            id: attempt_id,
            block,
            connection,
            issued_at: now,
            disposition: RequestDisposition::Requested,
        };
        if self.active_attempts.insert(attempt_id, attempt).is_some() {
            return Err(SwarmError::Invariant(
                "request attempt identifier was already active",
            ));
        }
        let connection_state = self.connection_mut(connection)?;
        connection_state.active_request_count =
            connection_state.active_request_count.checked_add(1).ok_or(
                SwarmError::ArithmeticOverflow("connection active request count"),
            )?;
        let length = usize::try_from(block.length)
            .map_err(|_| SwarmError::ArithmeticOverflow("block length"))?;
        self.outstanding_request_bytes = self
            .outstanding_request_bytes
            .checked_add(length)
            .ok_or(SwarmError::ArithmeticOverflow("request reservation"))?;
        self.outstanding_request_high_water = self
            .outstanding_request_high_water
            .max(self.outstanding_request_bytes);
        if endgame {
            self.endgame_assignments = self.endgame_assignments.saturating_add(1);
        }
        Ok(RequestAssignment {
            attempt: attempt_id,
            connection,
            block,
        })
    }

    fn terminate_requested(
        &mut self,
        block: BlockKey,
        attempt_id: RequestAttemptId,
        disposition: RequestDisposition,
    ) -> Result<(), SwarmError> {
        let (connection, became_missing) = {
            let state = self
                .blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?;
            if !matches!(state.phase, BlockPhase::Requested) {
                return Err(SwarmError::InvalidTransition(
                    "request attempt no longer owns the block",
                ));
            }
            let attempt = state
                .attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or(SwarmError::Invariant("active request attempt is missing"))?;
            if !attempt.disposition.is_active() {
                return Err(SwarmError::Invariant(
                    "active request has a terminal disposition",
                ));
            }
            let connection = attempt.connection;
            attempt.disposition = disposition;
            let became_missing = !state
                .attempts
                .iter()
                .any(|attempt| attempt.disposition.is_active());
            state.phase = if became_missing {
                BlockPhase::Missing
            } else {
                BlockPhase::Requested
            };
            trim_terminal_attempts(state, self.config.max_terminal_attempts_per_block, None)?;
            (connection, became_missing)
        };
        if self.active_attempts.remove(&attempt_id).is_none() {
            return Err(SwarmError::Invariant(
                "terminated request is absent from the active index",
            ));
        }
        self.release_connection_request(connection)?;
        if became_missing {
            self.requested_blocks = self
                .requested_blocks
                .checked_sub(1)
                .ok_or(SwarmError::Invariant("requested block count underflow"))?;
            self.missing_blocks = self
                .missing_blocks
                .checked_add(1)
                .ok_or(SwarmError::ArithmeticOverflow("missing block count"))?;
            self.note_piece_block_became_missing(block)?;
            self.deactivate_piece_if_idle(block.piece)?;
            self.refresh_requestable_piece(block.piece)?;
        }
        self.release_request(block.length)
    }

    fn release_requests_for_connection(
        &mut self,
        connection: ConnectionId,
        disposition: RequestDisposition,
    ) -> Result<Vec<BlockKey>, SwarmError> {
        let requested = self
            .active_attempts
            .values()
            .filter(|attempt| attempt.connection == connection)
            .map(|attempt| (attempt.block, attempt.id))
            .collect::<Vec<_>>();
        let mut released = Vec::with_capacity(requested.len());
        for (block, attempt) in requested {
            self.terminate_requested(block, attempt, disposition)?;
            released.push(block);
        }
        Ok(released)
    }

    fn release_request(&mut self, length: u32) -> Result<(), SwarmError> {
        let length =
            usize::try_from(length).map_err(|_| SwarmError::ArithmeticOverflow("block length"))?;
        self.outstanding_request_bytes = self
            .outstanding_request_bytes
            .checked_sub(length)
            .ok_or(SwarmError::Invariant("request reservation underflow"))?;
        Ok(())
    }

    fn release_connection_request(&mut self, connection: ConnectionId) -> Result<(), SwarmError> {
        let Some(connection) = self.connections.get_mut(&connection) else {
            return Err(SwarmError::Invariant(
                "active request refers to an absent connection",
            ));
        };
        connection.active_request_count =
            connection
                .active_request_count
                .checked_sub(1)
                .ok_or(SwarmError::Invariant(
                    "connection active request count underflow",
                ))?;
        self.picker.note_eligibility_change();
        Ok(())
    }

    fn note_unverified_contribution(&mut self, connection: ConnectionId) -> Result<(), SwarmError> {
        let count = self
            .unverified_contributor_blocks
            .entry(connection)
            .or_default();
        *count = count.checked_add(1).ok_or(SwarmError::ArithmeticOverflow(
            "unverified contributor block count",
        ))?;
        Ok(())
    }

    fn release_unverified_contribution(
        &mut self,
        connection: ConnectionId,
    ) -> Result<(), SwarmError> {
        let count = self
            .unverified_contributor_blocks
            .get_mut(&connection)
            .ok_or(SwarmError::Invariant(
                "unverified contributor is absent from the active index",
            ))?;
        *count = count.checked_sub(1).ok_or(SwarmError::Invariant(
            "unverified contributor block count underflow",
        ))?;
        if *count == 0 {
            self.unverified_contributor_blocks.remove(&connection);
        }
        Ok(())
    }

    fn activate_piece(&mut self, piece: u32) -> Result<(), SwarmError> {
        let index =
            usize::try_from(piece).map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
        if self.active_pieces.insert(index) {
            self.active_piece_bytes = self
                .active_piece_bytes
                .checked_add(self.piece_working_set_bytes(index))
                .ok_or(SwarmError::ArithmeticOverflow("active piece bytes"))?;
            self.last_activated_piece = Some(piece);
            self.last_activated_availability = self.picker.availability(index);
        }
        self.refresh_requestable_piece(piece)?;
        Ok(())
    }

    fn deactivate_piece(&mut self, piece: u32) -> Result<(), SwarmError> {
        let index =
            usize::try_from(piece).map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
        if !self.active_pieces.remove(&index) {
            return Ok(());
        }
        self.remove_requestable_piece(index)?;
        self.active_piece_bytes = self
            .active_piece_bytes
            .checked_sub(self.piece_working_set_bytes(index))
            .ok_or(SwarmError::Invariant("active piece byte count underflow"))?;
        Ok(())
    }

    fn refresh_requestable_piece(&mut self, piece: u32) -> Result<(), SwarmError> {
        let index =
            usize::try_from(piece).map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
        let piece_state = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        let next_missing = piece_state
            .blocks
            .get(piece_state.first_missing_block)
            .copied();
        if self.active_pieces.contains(&index)
            && let Some(block) = next_missing
        {
            let inserted_piece = self.requestable_active_pieces.insert(index);
            let inserted_block = self
                .requestable_active_block_keys
                .insert(index, block)
                .is_none();
            if inserted_piece != inserted_block {
                return Err(SwarmError::Invariant("requestable active indexes disagree"));
            }
            if inserted_piece {
                self.requestable_active_blocks = self
                    .requestable_active_blocks
                    .checked_add(
                        self.pieces
                            .get(&piece)
                            .ok_or(SwarmError::UnknownPiece(piece))?
                            .blocks
                            .len(),
                    )
                    .ok_or(SwarmError::ArithmeticOverflow(
                        "requestable active block span",
                    ))?;
            }
        } else {
            self.remove_requestable_piece(index)?;
        }
        Ok(())
    }

    fn remove_requestable_piece(&mut self, index: usize) -> Result<(), SwarmError> {
        if !self.requestable_active_pieces.remove(&index) {
            if self.requestable_active_block_keys.contains_key(&index) {
                return Err(SwarmError::Invariant(
                    "requestable active block exists without its piece",
                ));
            }
            return Ok(());
        }
        if self.requestable_active_block_keys.remove(&index).is_none() {
            return Err(SwarmError::Invariant(
                "requestable active piece has no cached block",
            ));
        }
        let piece =
            u32::try_from(index).map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
        self.requestable_active_blocks = self
            .requestable_active_blocks
            .checked_sub(
                self.pieces
                    .get(&piece)
                    .ok_or(SwarmError::UnknownPiece(piece))?
                    .blocks
                    .len(),
            )
            .ok_or(SwarmError::Invariant(
                "requestable active block span underflow",
            ))?;
        Ok(())
    }

    fn deactivate_piece_if_idle(&mut self, piece: u32) -> Result<(), SwarmError> {
        let active_blocks = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .active_blocks;
        if active_blocks == 0 {
            self.deactivate_piece(piece)?;
        }
        Ok(())
    }

    fn note_piece_block_assigned(&mut self, block: BlockKey) -> Result<(), SwarmError> {
        let piece = self
            .pieces
            .get(&block.piece)
            .ok_or(SwarmError::UnknownPiece(block.piece))?;
        let position = piece
            .blocks
            .binary_search(&block)
            .map_err(|_| SwarmError::UnknownBlock(block))?;
        if position != piece.first_missing_block {
            return Err(SwarmError::Invariant(
                "assigned block is not the cached first missing block",
            ));
        }
        let missing_blocks = piece
            .missing_blocks
            .checked_sub(1)
            .ok_or(SwarmError::Invariant("piece missing block count underflow"))?;
        let first_missing_block = if missing_blocks == 0 {
            piece.blocks.len()
        } else {
            piece
                .blocks
                .iter()
                .enumerate()
                .skip(position + 1)
                .find_map(|(index, candidate)| {
                    self.blocks
                        .get(candidate)
                        .is_some_and(|state| matches!(state.phase, BlockPhase::Missing))
                        .then_some(index)
                })
                .ok_or(SwarmError::Invariant(
                    "piece missing block cursor has no missing block",
                ))?
        };
        let piece = self
            .pieces
            .get_mut(&block.piece)
            .ok_or(SwarmError::UnknownPiece(block.piece))?;
        piece.missing_blocks = piece
            .missing_blocks
            .checked_sub(1)
            .ok_or(SwarmError::Invariant("piece missing block count underflow"))?;
        piece.active_blocks = piece
            .active_blocks
            .checked_add(1)
            .ok_or(SwarmError::ArithmeticOverflow("piece active block count"))?;
        piece.first_missing_block = first_missing_block;
        Ok(())
    }

    fn note_piece_block_became_missing(&mut self, block: BlockKey) -> Result<(), SwarmError> {
        let piece = self
            .pieces
            .get_mut(&block.piece)
            .ok_or(SwarmError::UnknownPiece(block.piece))?;
        let position = piece
            .blocks
            .binary_search(&block)
            .map_err(|_| SwarmError::UnknownBlock(block))?;
        piece.active_blocks = piece
            .active_blocks
            .checked_sub(1)
            .ok_or(SwarmError::Invariant("piece active block count underflow"))?;
        piece.missing_blocks = piece
            .missing_blocks
            .checked_add(1)
            .ok_or(SwarmError::ArithmeticOverflow("piece missing block count"))?;
        piece.first_missing_block = piece.first_missing_block.min(position);
        Ok(())
    }

    fn connection_request_count(&self, connection: ConnectionId) -> usize {
        self.connections
            .get(&connection)
            .map_or(0, |state| state.active_request_count)
    }

    fn has_unique_wanted_piece(&self, connection: ConnectionId) -> bool {
        let Some(current) = self.connections.get(&connection) else {
            return false;
        };
        current
            .availability
            .set_pieces()
            .any(|piece| self.picker.is_wanted(piece) && self.picker.availability(piece) == Some(1))
    }

    fn no_request_reason(&self) -> Option<NoRequestReason> {
        if self.is_complete() {
            return Some(NoRequestReason::Complete);
        }
        if self.connections.is_empty() {
            return Some(NoRequestReason::NoConnections);
        }
        let mut useful = self
            .connections
            .values()
            .filter(|connection| connection.wanted_piece_count != 0);
        let useful_count = useful.clone().count();
        if useful_count == 0 {
            return Some(NoRequestReason::NoPeerHasWantedPiece);
        }
        if useful.all(|connection| connection.choking) {
            return Some(NoRequestReason::AllUsefulPeersChoked);
        }
        if self.outstanding_request_bytes >= self.config.max_outstanding_request_bytes {
            return Some(NoRequestReason::OutstandingRequestLimit);
        }
        if self.connections.keys().all(|connection| {
            self.connections.get(connection).is_some_and(|state| {
                self.connection_request_count(*connection) >= state.request_window.target
            })
        }) {
            return Some(NoRequestReason::RequestWindowsFull);
        }
        let blocked_by_working_set = !self.incomplete_pieces.is_empty()
            && self.incomplete_pieces.iter().all(|piece| {
                self.active_pieces.contains(piece) || !self.can_activate_piece(*piece)
            });
        if blocked_by_working_set {
            return Some(NoRequestReason::ActivePieceLimit);
        }
        None
    }
}

fn trim_terminal_attempts(
    state: &mut BlockState,
    maximum: usize,
    protected: Option<RequestAttemptId>,
) -> Result<(), SwarmError> {
    while state
        .attempts
        .iter()
        .filter(|attempt| !attempt.disposition.is_active())
        .count()
        > maximum
    {
        let Some(index) = state
            .attempts
            .iter()
            .position(|attempt| !attempt.disposition.is_active() && Some(attempt.id) != protected)
        else {
            return Err(SwarmError::Invariant(
                "terminal attempt count cannot be reduced",
            ));
        };
        state.attempts.remove(index);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwarmError {
    InvalidConfig(&'static str),
    InvalidBlock {
        piece: u32,
        begin: u32,
        length: u32,
    },
    EmptyPiecePlan(u32),
    OverlappingBlocks(u32),
    PieceOutOfRange {
        piece: u32,
        piece_count: usize,
    },
    DuplicatePiecePlan(u32),
    DuplicateBlock(BlockKey),
    UnknownPiece(u32),
    UnknownBlock(BlockKey),
    StalePieceGeneration {
        expected: PieceGeneration,
        actual: PieceGeneration,
    },
    DuplicatePendingDial(PendingDialId),
    UnknownPendingDial(PendingDialId),
    PendingDialCapacity,
    DuplicateConnection(ConnectionId),
    UnknownConnection(ConnectionId),
    ConnectionCapacity,
    InvalidAvailability {
        actual: usize,
        expected: usize,
    },
    InvalidTransition(&'static str),
    IdentifierOverflow(&'static str),
    ArithmeticOverflow(&'static str),
    Invariant(&'static str),
}

impl fmt::Display for SwarmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => {
                write!(formatter, "invalid swarm configuration: {message}")
            }
            Self::InvalidBlock {
                piece,
                begin,
                length,
            } => write!(
                formatter,
                "invalid block piece={piece} begin={begin} length={length}"
            ),
            Self::EmptyPiecePlan(piece) => write!(formatter, "piece {piece} has no wanted blocks"),
            Self::OverlappingBlocks(piece) => {
                write!(formatter, "piece {piece} has overlapping wanted blocks")
            }
            Self::PieceOutOfRange { piece, piece_count } => {
                write!(
                    formatter,
                    "piece {piece} is outside piece count {piece_count}"
                )
            }
            Self::DuplicatePiecePlan(piece) => write!(formatter, "piece {piece} has two plans"),
            Self::DuplicateBlock(block) => write!(formatter, "duplicate block {block:?}"),
            Self::UnknownPiece(piece) => write!(formatter, "unknown piece {piece}"),
            Self::UnknownBlock(block) => write!(formatter, "unknown block {block:?}"),
            Self::StalePieceGeneration { expected, actual } => write!(
                formatter,
                "stale piece generation {}, expected {}",
                actual.get(),
                expected.get()
            ),
            Self::DuplicatePendingDial(id) => {
                write!(formatter, "duplicate pending dial {}", id.get())
            }
            Self::UnknownPendingDial(id) => write!(formatter, "unknown pending dial {}", id.get()),
            Self::PendingDialCapacity => formatter.write_str("pending dial capacity is full"),
            Self::DuplicateConnection(id) => write!(formatter, "duplicate connection {}", id.get()),
            Self::UnknownConnection(id) => write!(formatter, "unknown connection {}", id.get()),
            Self::ConnectionCapacity => {
                formatter.write_str("established connection capacity is full")
            }
            Self::InvalidAvailability { actual, expected } => write!(
                formatter,
                "availability has {actual} pieces, expected {expected}"
            ),
            Self::InvalidTransition(message) => {
                write!(formatter, "invalid swarm transition: {message}")
            }
            Self::IdentifierOverflow(owner) => write!(formatter, "{owner} identifier overflow"),
            Self::ArithmeticOverflow(owner) => write!(formatter, "{owner} arithmetic overflow"),
            Self::Invariant(message) => write!(formatter, "swarm invariant failed: {message}"),
        }
    }
}

impl Error for SwarmError {}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: u32 = MAX_REQUEST_BLOCK_LENGTH;

    fn connection(value: u64) -> ConnectionId {
        ConnectionId::new(value).expect("nonzero connection")
    }

    fn dial(value: u64) -> PendingDialId {
        PendingDialId::new(value).expect("nonzero dial")
    }

    fn plan(piece: u32, blocks: usize) -> PiecePlan {
        PiecePlan::new(
            piece,
            &(0..blocks)
                .map(|index| (u32::try_from(index).expect("small index") * BLOCK, BLOCK))
                .collect::<Vec<_>>(),
        )
        .expect("piece plan")
    }

    fn state(piece_count: usize, plans: Vec<PiecePlan>, payload_blocks: usize) -> SwarmState {
        let mut config = SwarmConfig::for_request_limit(payload_blocks * BLOCK as usize);
        config.request_timeout = Duration::from_secs(30);
        config.unproductive_grace = Duration::from_secs(30);
        SwarmState::new(config, piece_count, plans).expect("swarm state")
    }

    fn add_peer(state: &mut SwarmState, id: ConnectionId, pieces: &[usize], choking: bool) {
        state.add_connection(id, Duration::ZERO).expect("peer");
        let mut availability = vec![false; state.piece_count];
        for &piece in pieces {
            availability[piece] = true;
        }
        state.set_bitfield(id, availability).expect("bitfield");
        state.set_choking(id, choking).expect("choke state");
    }

    fn assert_cached_indexes(state: &SwarmState) {
        let mut phases = [0_usize; 5];
        let mut active_attempts = BTreeMap::new();
        let mut connection_requests = BTreeMap::<ConnectionId, usize>::new();
        let mut unverified_contributor_blocks = BTreeMap::<ConnectionId, usize>::new();
        for (block, block_state) in &state.blocks {
            phases[match block_state.phase {
                BlockPhase::Missing => 0,
                BlockPhase::Requested => 1,
                BlockPhase::Writing { .. } => 2,
                BlockPhase::Received { .. } => 3,
                BlockPhase::Verified => 4,
            }] += 1;
            if let BlockPhase::Writing { source, .. } | BlockPhase::Received { source, .. } =
                block_state.phase
            {
                *unverified_contributor_blocks.entry(source).or_default() += 1;
            }
            for attempt in block_state
                .attempts
                .iter()
                .filter(|attempt| attempt.disposition.is_active())
            {
                assert_eq!(attempt.block, *block);
                assert_eq!(active_attempts.insert(attempt.id, *attempt), None);
                *connection_requests.entry(attempt.connection).or_default() += 1;
            }
        }
        assert_eq!(state.missing_blocks, phases[0]);
        assert_eq!(state.requested_blocks, phases[1]);
        assert_eq!(state.writing_blocks, phases[2]);
        assert_eq!(state.received_blocks, phases[3]);
        assert_eq!(state.verified_blocks, phases[4]);
        assert_eq!(state.active_attempts, active_attempts);
        assert_eq!(
            state.unverified_contributor_blocks,
            unverified_contributor_blocks
        );
        assert_eq!(
            state.outstanding_request_bytes,
            active_attempts
                .values()
                .map(|attempt| attempt.block.length as usize)
                .sum::<usize>()
        );
        for (id, connection) in &state.connections {
            assert_eq!(
                connection.active_request_count,
                connection_requests.get(id).copied().unwrap_or(0)
            );
        }
        for piece_state in state.pieces.values() {
            let mut missing_blocks = 0;
            let mut active_blocks = 0;
            let mut first_missing_block = piece_state.blocks.len();
            for (index, block) in piece_state.blocks.iter().enumerate() {
                match state.blocks.get(block).expect("planned block").phase {
                    BlockPhase::Missing => {
                        missing_blocks += 1;
                        first_missing_block = first_missing_block.min(index);
                    }
                    BlockPhase::Requested
                    | BlockPhase::Writing { .. }
                    | BlockPhase::Received { .. } => active_blocks += 1,
                    BlockPhase::Verified => {}
                }
            }
            assert_eq!(piece_state.missing_blocks, missing_blocks);
            assert_eq!(piece_state.active_blocks, active_blocks);
            assert_eq!(piece_state.first_missing_block, first_missing_block);
        }

        let incomplete_pieces = state
            .pieces
            .iter()
            .filter(|(_, piece_state)| {
                piece_state.blocks.iter().any(|block| {
                    !matches!(
                        state.blocks.get(block).map(|block| block.phase),
                        Some(BlockPhase::Verified)
                    )
                })
            })
            .map(|(&piece, _)| usize::try_from(piece).expect("piece index"))
            .collect::<BTreeSet<_>>();
        let active_pieces = state
            .pieces
            .iter()
            .filter(|(_, piece_state)| {
                piece_state.blocks.iter().any(|block| {
                    state.blocks.get(block).is_some_and(|block| {
                        matches!(
                            block.phase,
                            BlockPhase::Requested
                                | BlockPhase::Writing { .. }
                                | BlockPhase::Received { .. }
                        )
                    })
                })
            })
            .map(|(&piece, _)| usize::try_from(piece).expect("piece index"))
            .collect::<BTreeSet<_>>();
        assert_eq!(state.incomplete_pieces, incomplete_pieces);
        assert_eq!(state.active_pieces, active_pieces);
        let requestable_active_pieces = active_pieces
            .iter()
            .copied()
            .filter(|piece| {
                let piece = u32::try_from(*piece).expect("piece index");
                state.pieces[&piece].blocks.iter().any(|block| {
                    state
                        .blocks
                        .get(block)
                        .is_some_and(|block| matches!(block.phase, BlockPhase::Missing))
                })
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(state.requestable_active_pieces, requestable_active_pieces);
        let requestable_active_block_keys = requestable_active_pieces
            .iter()
            .map(|&piece| {
                let piece_u32 = u32::try_from(piece).expect("piece index");
                let state = &state.pieces[&piece_u32];
                (piece, state.blocks[state.first_missing_block])
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            state.requestable_active_block_keys,
            requestable_active_block_keys
        );
        assert_eq!(
            state.requestable_active_blocks,
            requestable_active_pieces
                .iter()
                .map(|piece| state.pieces[&(*piece as u32)].blocks.len())
                .sum::<usize>()
        );
        assert_eq!(
            state.active_piece_bytes,
            active_pieces
                .iter()
                .map(|piece| state.piece_working_set_bytes(*piece))
                .sum::<usize>()
        );
        for piece in 0..state.piece_count {
            let expected = state
                .connections
                .values()
                .filter(|connection| connection.availability.contains(piece))
                .count() as u32;
            assert_eq!(state.picker.availability(piece), Some(expected));
        }
        for connection in state.connections.values() {
            assert_eq!(
                connection.wanted_piece_count,
                connection
                    .availability
                    .set_pieces()
                    .filter(|piece| state.picker.is_wanted(*piece))
                    .count()
            );
        }
    }

    #[test]
    fn piece_storage_join_is_generation_safe_in_every_completion_order() {
        let generation = PieceGeneration::new(1).expect("initial generation");

        let mut hash_first = PieceStorageJoin::new(2);
        hash_first.begin_hash(generation).expect("begin early hash");
        assert_eq!(
            hash_first
                .hash_completed(generation, true)
                .expect("complete hash"),
            PieceStorageOutcome::Pending
        );
        assert_eq!(
            hash_first
                .write_completed(generation, true)
                .expect("first write"),
            PieceStorageOutcome::Pending
        );
        assert_eq!(
            hash_first
                .write_completed(generation, true)
                .expect("second write"),
            PieceStorageOutcome::Verified
        );

        let mut write_failure = PieceStorageJoin::new(2);
        write_failure
            .begin_hash(generation)
            .expect("begin failure join hash");
        write_failure
            .hash_completed(generation, true)
            .expect("hash passes");
        write_failure
            .write_completed(generation, false)
            .expect("failed write");
        assert_eq!(
            write_failure
                .write_completed(generation, true)
                .expect("joined write"),
            PieceStorageOutcome::Failed
        );

        let mut hash_failure = PieceStorageJoin::new(1);
        hash_failure
            .begin_hash(generation)
            .expect("begin failed hash");
        assert_eq!(
            hash_failure
                .hash_completed(generation, false)
                .expect("failed hash"),
            PieceStorageOutcome::Pending
        );
        assert_eq!(
            hash_failure
                .write_completed(generation, true)
                .expect("join final write"),
            PieceStorageOutcome::Failed
        );

        hash_failure.reset().expect("new generation");
        let replacement = PieceGeneration::new(2).expect("replacement generation");
        assert_eq!(hash_failure.generation, replacement);
        assert!(matches!(
            hash_failure.write_completed(generation, true),
            Err(SwarmError::StalePieceGeneration {
                expected,
                actual,
            }) if expected == replacement && actual == generation
        ));
        hash_failure
            .write_completed(replacement, true)
            .expect("replacement write");
        assert!(matches!(
            hash_failure.write_completed(replacement, true),
            Err(SwarmError::Invariant(_))
        ));

        let mut overflow = PieceStorageJoin::new(1);
        overflow.generation = PieceGeneration(u64::MAX);
        assert_eq!(
            overflow.reset(),
            Err(SwarmError::IdentifierOverflow("piece generation"))
        );
    }

    #[test]
    fn validates_plans_configuration_and_pending_dial_capacity() {
        assert!(matches!(
            PiecePlan::new(0, &[]),
            Err(SwarmError::EmptyPiecePlan(0))
        ));
        assert!(matches!(
            PiecePlan::new(0, &[(0, BLOCK), (BLOCK - 1, BLOCK)]),
            Err(SwarmError::OverlappingBlocks(0))
        ));
        let mut state = state(1, vec![plan(0, 1)], 1);
        for value in 1..=DEFAULT_MAX_PENDING_DIALS as u64 {
            state.begin_dial(dial(value)).expect("dial slot");
        }
        let overflow = DEFAULT_MAX_PENDING_DIALS as u64 + 1;
        assert_eq!(
            state.begin_dial(dial(overflow)),
            Err(SwarmError::PendingDialCapacity)
        );
        state.finish_dial(dial(1)).expect("finish dial");
        state.begin_dial(dial(overflow)).expect("reused dial slot");
        assert_eq!(state.snapshot(Duration::ZERO).pending_dials, 30);
    }

    #[test]
    fn connection_activity_exposes_bounded_queue_and_utility_state() {
        let mut state = state(1, vec![plan(0, 2)], 2);
        add_peer(&mut state, connection(1), &[0], false);
        add_peer(&mut state, connection(2), &[], true);
        let assignments = state.schedule(Duration::ZERO).expect("schedule");
        assert_eq!(assignments.len(), 2);
        assert!(matches!(
            state
                .receive_block(
                    assignments[0].connection,
                    assignments[0].block,
                    Duration::from_millis(100),
                )
                .expect("payload"),
            ReceiveDisposition::Accept { .. }
        ));
        state
            .finish_write(assignments[0].block, true, Duration::from_millis(100))
            .expect("write");

        let activity = state.connection_activity(Duration::from_secs(1));
        assert_eq!(activity.len(), 2);
        assert_eq!(activity[0].id, connection(1));
        assert!(!activity[0].choking);
        assert_eq!(activity[0].wanted_piece_count, 1);
        assert_eq!(activity[0].pending_requests, 1);
        assert_eq!(activity[0].target_requests, 5);
        assert_eq!(activity[0].queued_payload_bytes, BLOCK as usize);
        assert_eq!(
            activity[0].window_phase,
            ConnectionWindowPhaseSnapshot::SlowStart
        );
        assert_eq!(activity[0].useful_payload_bytes, BLOCK as usize);
        assert_eq!(
            activity[0].last_useful_age,
            Some(Duration::from_millis(900))
        );
        assert_eq!(
            activity[0].last_payload_age,
            Some(Duration::from_millis(900))
        );
        assert_eq!(activity[0].request_timeout, Duration::from_secs(2));
        assert_eq!(activity[0].oldest_request_age, Some(Duration::from_secs(1)));
        assert_eq!(activity[1].wanted_piece_count, 0);
        assert_eq!(activity[1].pending_requests, 0);
    }

    #[test]
    fn distributes_requests_fairly_and_holds_the_global_request_bound() {
        let mut state = state(1, vec![plan(0, 8)], 4);
        for id in [connection(1), connection(2)] {
            add_peer(&mut state, id, &[0], false);
        }
        let assignments = state.schedule(Duration::ZERO).expect("schedule");
        assert_eq!(assignments.len(), 4);
        assert_eq!(
            assignments
                .iter()
                .map(|assignment| assignment.connection)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([connection(1), connection(2)])
        );
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.outstanding_request_bytes, 4 * BLOCK as usize);
        assert_eq!(
            snapshot.outstanding_request_high_water,
            snapshot.outstanding_request_bytes
        );
        assert_eq!(
            snapshot.no_request_reason,
            Some(NoRequestReason::OutstandingRequestLimit)
        );
    }

    #[test]
    fn generous_request_allowance_fills_every_default_initial_peer_window() {
        let peer_count = DEFAULT_MAX_ESTABLISHED_CONNECTIONS;
        let block_count = peer_count * DEFAULT_INITIAL_REQUESTS_PER_CONNECTION;
        let config = SwarmConfig::for_request_limit(256 * 1024 * 1024);
        let mut state = SwarmState::new(config, 1, vec![plan(0, block_count)]).expect("swarm");
        for value in 1..=peer_count as u64 {
            add_peer(&mut state, connection(value), &[0], false);
        }

        let assignments = state.schedule(Duration::ZERO).expect("schedule");

        assert_eq!(assignments.len(), block_count);
        for value in 1..=peer_count as u64 {
            assert_eq!(
                assignments
                    .iter()
                    .filter(|assignment| assignment.connection == connection(value))
                    .count(),
                DEFAULT_INITIAL_REQUESTS_PER_CONNECTION
            );
        }
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(
            snapshot.outstanding_request_bytes,
            block_count * BLOCK as usize
        );
        assert_eq!(
            snapshot.outstanding_request_high_water,
            snapshot.outstanding_request_bytes
        );
    }

    #[test]
    fn active_piece_working_set_is_byte_bounded_and_refills_after_verification() {
        let mut config = SwarmConfig::for_request_limit(4 * BLOCK as usize);
        config.max_active_piece_bytes = 2 * BLOCK as usize;
        let mut state = SwarmState::new(config, 4, (0..4).map(|piece| plan(piece, 1)).collect())
            .expect("swarm");
        add_peer(&mut state, connection(1), &[0, 1, 2, 3], false);

        let initial = state.schedule(Duration::ZERO).expect("initial schedule");
        assert_eq!(initial.len(), 2);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.active_piece_count, 2);
        assert_eq!(snapshot.active_piece_bytes, 2 * BLOCK as usize);
        assert_eq!(
            snapshot.no_request_reason,
            Some(NoRequestReason::ActivePieceLimit)
        );

        state
            .receive_block(connection(1), initial[0].block, Duration::ZERO)
            .expect("payload");
        state
            .finish_write(initial[0].block, true, Duration::ZERO)
            .expect("write");
        state
            .mark_piece_verified(initial[0].block.piece)
            .expect("verify");
        let refill = state.schedule(Duration::from_millis(1)).expect("refill");
        assert_eq!(refill.len(), 1);
        let refilled = state.snapshot(Duration::from_millis(1));
        assert_eq!(refilled.active_piece_count, 2);
        assert_eq!(refilled.active_piece_bytes, 2 * BLOCK as usize);
    }

    #[test]
    fn one_piece_larger_than_the_working_set_limit_can_still_progress() {
        let mut config = SwarmConfig::for_request_limit(2 * BLOCK as usize);
        config.max_active_piece_bytes = BLOCK as usize;
        let mut state = SwarmState::new(config, 1, vec![plan(0, 2)]).expect("swarm");
        add_peer(&mut state, connection(1), &[0], false);

        let assignments = state.schedule(Duration::ZERO).expect("schedule");

        assert_eq!(assignments.len(), 2);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.active_piece_count, 1);
        assert_eq!(snapshot.active_piece_bytes, 2 * BLOCK as usize);
    }

    #[test]
    fn useful_payload_grows_and_refills_only_the_responding_peer_window() {
        let mut state = state(1, vec![plan(0, 32)], 32);
        for id in [connection(1), connection(2)] {
            add_peer(&mut state, id, &[0], false);
        }
        let initial = state.schedule(Duration::ZERO).expect("initial schedule");
        assert_eq!(initial.len(), 2 * DEFAULT_INITIAL_REQUESTS_PER_CONNECTION);
        assert_eq!(
            initial
                .iter()
                .filter(|request| request.connection == connection(1))
                .count(),
            DEFAULT_INITIAL_REQUESTS_PER_CONNECTION
        );
        let response = initial
            .iter()
            .find(|request| request.connection == connection(1))
            .expect("peer one request");
        state
            .receive_block(
                response.connection,
                response.block,
                Duration::from_millis(100),
            )
            .expect("useful payload");
        state
            .finish_write(response.block, true, Duration::from_millis(100))
            .expect("stored block");

        let refill = state
            .schedule(Duration::from_millis(100))
            .expect("refill schedule");
        assert_eq!(refill.len(), 2);
        assert!(
            refill
                .iter()
                .all(|request| request.connection == connection(1))
        );
        let snapshot = state.snapshot(Duration::from_millis(100));
        assert_eq!(snapshot.request_target_total, 9);
        assert_eq!(snapshot.request_target_max, 5);
        assert_eq!(snapshot.slow_start_peers, 2);
        assert_eq!(snapshot.stalled_peers, 0);
        assert_eq!(snapshot.useful_payload_bytes, BLOCK as usize);
    }

    #[test]
    fn slow_start_settles_on_a_bounded_three_second_rate_target() {
        let config = SwarmConfig::for_request_limit(64 * BLOCK as usize);
        let mut window = RequestWindow::new(Duration::ZERO, config.initial_requests_per_connection);
        window.accepted_payload(
            Duration::from_millis(100),
            BLOCK as usize,
            Some(Duration::ZERO),
            config,
        );
        assert_eq!(window.target, 5);
        window.accepted_payload(
            Duration::from_millis(1_100),
            BLOCK as usize,
            Some(Duration::from_secs(1)),
            config,
        );
        assert_eq!(window.phase, RequestWindowPhase::SlowStart);
        assert_eq!(window.target, 6);

        window.refresh(Duration::from_millis(2_100), config);
        assert_eq!(window.phase, RequestWindowPhase::Steady);
        assert_eq!(window.observed_payload_rate, BLOCK as usize);
        assert_eq!(window.target, 3);
        assert_eq!(
            rate_target(usize::MAX, config),
            DEFAULT_MAX_REQUESTS_PER_CONNECTION
        );
    }

    #[test]
    fn useful_response_samples_bound_connection_inactivity_timeout() {
        let config = SwarmConfig::for_request_limit(16 * BLOCK as usize);
        let mut window = RequestWindow::new(Duration::ZERO, config.initial_requests_per_connection);
        assert_eq!(window.request_timeout(config), DEFAULT_REQUEST_TIMEOUT);
        window.accepted_payload(
            Duration::from_millis(100),
            BLOCK as usize,
            Some(Duration::ZERO),
            config,
        );
        assert_eq!(window.request_timeout(config), Duration::from_secs(2));
        window.accepted_payload(
            Duration::from_millis(200),
            BLOCK as usize,
            Some(Duration::from_millis(100)),
            config,
        );
        assert_eq!(window.request_timeout(config), Duration::from_secs(2));

        let mut short_config = config;
        short_config.request_timeout = Duration::from_millis(75);
        assert_eq!(
            window.request_timeout(short_config),
            Duration::from_millis(75)
        );
    }

    #[test]
    fn sampled_connection_stall_releases_its_whole_window() {
        let mut state = state(1, vec![plan(0, 16)], 16);
        add_peer(&mut state, connection(1), &[0], false);
        let initial = state.schedule(Duration::ZERO).expect("initial");
        state
            .receive_block(
                initial[0].connection,
                initial[0].block,
                Duration::from_millis(100),
            )
            .expect("first response");
        state
            .finish_write(initial[0].block, true, Duration::from_millis(100))
            .expect("first stored");
        let refill = state
            .schedule(Duration::from_millis(100))
            .expect("expanded refill");
        assert_eq!(refill.len(), 2);
        assert!(
            state
                .expire_requests(Duration::from_millis(2_099))
                .expect("before adaptive deadline")
                .is_empty()
        );
        let expired = state
            .expire_requests(Duration::from_millis(2_100))
            .expect("adaptive deadline");
        assert_eq!(expired.len(), 5);
        let snapshot = state.snapshot(Duration::from_millis(2_100));
        assert_eq!(snapshot.requested_blocks, 0);
        assert_eq!(snapshot.outstanding_request_bytes, 0);
        assert_eq!(snapshot.request_target_total, 1);
        assert_eq!(snapshot.stalled_peers, 1);
        assert_eq!(snapshot.request_timeout_min, Some(Duration::from_secs(2)));
    }

    #[test]
    fn expired_window_probes_once_and_recovers_on_requested_payload() {
        let mut state = state(1, vec![plan(0, 16)], 16);
        add_peer(&mut state, connection(1), &[0], false);
        assert_eq!(
            state.schedule(Duration::ZERO).expect("initial").len(),
            DEFAULT_INITIAL_REQUESTS_PER_CONNECTION
        );
        assert_eq!(
            state
                .expire_requests(Duration::from_secs(30))
                .expect("expire")
                .len(),
            DEFAULT_INITIAL_REQUESTS_PER_CONNECTION
        );
        let stalled = state.snapshot(Duration::from_secs(30));
        assert_eq!(stalled.request_target_total, 1);
        assert_eq!(stalled.stalled_peers, 1);

        let probe = state.schedule(Duration::from_secs(30)).expect("probe");
        assert_eq!(probe.len(), 1);
        state
            .receive_block(probe[0].connection, probe[0].block, Duration::from_secs(31))
            .expect("probe payload");
        state
            .finish_write(probe[0].block, true, Duration::from_secs(31))
            .expect("probe stored");
        let refill = state.schedule(Duration::from_secs(31)).expect("recovered");
        assert_eq!(refill.len(), MIN_HEALTHY_REQUESTS_PER_CONNECTION);
        let recovered = state.snapshot(Duration::from_secs(31));
        assert_eq!(
            recovered.request_target_total,
            MIN_HEALTHY_REQUESTS_PER_CONNECTION
        );
        assert_eq!(recovered.stalled_peers, 0);
        assert_eq!(recovered.outstanding_request_bytes, 2 * BLOCK as usize);
    }

    #[test]
    fn choke_releases_only_that_connection_requests() {
        let mut state = state(1, vec![plan(0, 4)], 4);
        add_peer(&mut state, connection(1), &[0], false);
        add_peer(&mut state, connection(2), &[0], false);
        let assigned = state.schedule(Duration::ZERO).expect("schedule");
        let peer_one = assigned
            .iter()
            .filter(|request| request.connection == connection(1))
            .count();
        let released = state.set_choking(connection(1), true).expect("peer choke");
        assert_eq!(released.len(), peer_one);
        assert!(
            state
                .attempts_for(released[0])
                .expect("attempts")
                .iter()
                .any(|attempt| attempt.disposition == RequestDisposition::Choked)
        );
        assert_eq!(
            state.snapshot(Duration::ZERO).requested_blocks,
            assigned.len() - peer_one
        );
    }

    #[test]
    fn fast_choke_retains_requests_and_exact_reject_refills_immediately() {
        let mut state = state(1, vec![plan(0, 4)], 4);
        add_peer(&mut state, connection(1), &[0], false);
        add_peer(&mut state, connection(2), &[0], false);
        state
            .set_fast_extension(connection(1), true)
            .expect("enable Fast before requests");
        let assigned = state.schedule(Duration::ZERO).expect("schedule");
        let rejected = assigned
            .iter()
            .find(|request| request.connection == connection(1))
            .copied()
            .expect("Fast peer request");
        let before = state.snapshot(Duration::ZERO);

        assert!(
            state
                .set_choking(connection(1), true)
                .expect("Fast choke")
                .is_empty()
        );
        assert_eq!(
            state.snapshot(Duration::ZERO).requested_blocks,
            before.requested_blocks
        );
        assert_eq!(
            state
                .reject_request(connection(1), rejected.block)
                .expect("exact reject"),
            RejectDisposition::Accepted {
                attempt: rejected.attempt
            }
        );
        let after_reject = state.snapshot(Duration::ZERO);
        assert_eq!(
            after_reject.outstanding_request_bytes,
            before.outstanding_request_bytes - BLOCK as usize
        );
        let refill = state.schedule(Duration::ZERO).expect("immediate refill");
        assert_eq!(refill.len(), 1);
        assert_eq!(refill[0].connection, connection(2));
        assert_eq!(refill[0].block, rejected.block);
        assert_eq!(
            state
                .reject_request(connection(1), rejected.block)
                .expect("stale duplicate reject"),
            RejectDisposition::Stale
        );
        assert_eq!(
            state
                .reject_request(
                    connection(1),
                    BlockKey::new(0, 4 * BLOCK, BLOCK).expect("valid never-sent block")
                )
                .expect("never-sent reject"),
            RejectDisposition::NeverRequested
        );
    }

    #[test]
    fn fast_initial_availability_and_advisories_are_strict_and_bounded() {
        let mut state = state(64, vec![plan(0, 1)], 1);
        let peer = connection(1);
        state.add_connection(peer, Duration::ZERO).expect("peer");
        state
            .set_fast_extension(peer, true)
            .expect("enable Fast before messages");
        state
            .observe_fast_message(peer, &PeerMessage::KeepAlive)
            .expect("capability-neutral control may precede availability");
        assert!(
            state
                .observe_fast_message(peer, &PeerMessage::SuggestPiece(0))
                .is_err()
        );
        state
            .observe_fast_message(peer, &PeerMessage::HaveNone)
            .expect("initial have-none");
        state.set_have_none(peer).expect("apply have-none");
        assert!(
            state
                .observe_fast_message(peer, &PeerMessage::HaveAll)
                .is_err()
        );
        for piece in 0..64 {
            state
                .observe_fast_message(peer, &PeerMessage::SuggestPiece(piece))
                .expect("bounded suggestion");
            state
                .observe_fast_message(peer, &PeerMessage::AllowedFast(piece))
                .expect("bounded allowed-fast");
        }
        for _ in 0..8 {
            state
                .observe_fast_message(peer, &PeerMessage::SuggestPiece(0))
                .expect("duplicate suggestion");
            state
                .observe_fast_message(peer, &PeerMessage::AllowedFast(0))
                .expect("duplicate allowed-fast");
        }
        assert_eq!(
            state.fast_peer_snapshot(peer).expect("Fast snapshot"),
            FastPeerSnapshot {
                negotiated: true,
                initial_availability_received: true,
                suggestions: MAX_FAST_ADVISORY_PIECES,
                allowed_fast: MAX_FAST_ADVISORY_PIECES,
            }
        );
        assert!(
            state
                .observe_fast_message(peer, &PeerMessage::SuggestPiece(64))
                .is_err()
        );
    }

    #[test]
    fn received_allowed_fast_permits_only_that_piece_while_choked() {
        let mut state = state(2, vec![plan(0, 1), plan(1, 1)], 2);
        let peer = connection(1);
        state.add_connection(peer, Duration::ZERO).expect("peer");
        state
            .set_fast_extension(peer, true)
            .expect("enable Fast before messages");
        let bitfield = PeerMessage::Bitfield(vec![0b1100_0000]);
        state
            .observe_fast_message(peer, &bitfield)
            .expect("initial bitfield");
        state
            .set_compact_bitfield(peer, vec![0b1100_0000])
            .expect("apply initial bitfield");
        state
            .observe_fast_message(peer, &PeerMessage::AllowedFast(1))
            .expect("allowed-fast piece");
        let assignment = state
            .schedule(Duration::ZERO)
            .expect("choked Fast schedule");
        assert_eq!(assignment.len(), 1);
        assert_eq!(assignment[0].block.piece, 1);
        assert_eq!(
            state
                .reject_request(peer, assignment[0].block)
                .expect("allowed-fast reject"),
            RejectDisposition::Accepted {
                attempt: assignment[0].attempt
            }
        );
        assert!(
            state
                .schedule(Duration::ZERO)
                .expect("no remaining Fast work")
                .is_empty()
        );
    }

    #[test]
    fn request_expiry_is_independent_from_other_peer_activity() {
        let mut state = state(1, vec![plan(0, 1)], 1);
        add_peer(&mut state, connection(1), &[0], false);
        let request = state.schedule(Duration::ZERO).expect("schedule")[0];
        state
            .peer_has(connection(1), 0)
            .expect("unrelated valid activity");
        assert!(
            state
                .expire_requests(Duration::from_secs(29))
                .expect("not expired")
                .is_empty()
        );
        let expired = state
            .expire_requests(Duration::from_secs(30))
            .expect("expire");
        assert_eq!(expired[0].attempt, request.attempt);
        assert_eq!(
            state
                .snapshot(Duration::from_secs(30))
                .outstanding_request_bytes,
            0
        );
    }

    #[test]
    fn storage_gating_defers_request_and_replacement_deadlines() {
        let mut state = state(1, vec![plan(0, 1)], 1);
        add_peer(&mut state, connection(1), &[0], false);
        let request = state.schedule(Duration::ZERO).expect("schedule")[0];
        state.defer_peer_deadlines(Duration::from_secs(20));
        assert!(
            state
                .expire_requests(Duration::from_secs(30))
                .expect("deferred request remains live")
                .is_empty()
        );
        let snapshot = state.snapshot(Duration::from_secs(30));
        assert_eq!(snapshot.oldest_request_age, Some(Duration::from_secs(10)));
        let expired = state
            .expire_requests(Duration::from_secs(50))
            .expect("deferred request expires");
        assert_eq!(expired[0].attempt, request.attempt);
    }

    #[test]
    fn late_response_after_reassignment_is_safe_and_accounted_once() {
        let mut state = state(1, vec![plan(0, 1)], 1);
        add_peer(&mut state, connection(1), &[0], false);
        add_peer(&mut state, connection(2), &[0], false);
        let first = state.schedule(Duration::ZERO).expect("first request")[0];
        state
            .expire_requests(Duration::from_secs(30))
            .expect("expire first");
        state.set_choking(connection(1), true).expect("choke old");
        let second = state.schedule(Duration::from_secs(30)).expect("reassign")[0];
        assert_ne!(first.connection, second.connection);
        assert_eq!(
            state
                .receive_block(first.connection, first.block, Duration::from_secs(30))
                .expect("late block"),
            ReceiveDisposition::Accept {
                evidence: first.attempt,
                cancellations: vec![RequestCancellation {
                    attempt: second.attempt,
                    connection: second.connection,
                    block: second.block,
                }],
                late: true,
            }
        );
        assert_eq!(
            state
                .snapshot(Duration::from_secs(30))
                .outstanding_request_bytes,
            0
        );
        state
            .finish_write(first.block, true, Duration::from_secs(30))
            .expect("stored");
        assert_eq!(
            state
                .snapshot(Duration::from_secs(30))
                .outstanding_request_bytes,
            0
        );
        assert_eq!(
            state
                .receive_block(second.connection, second.block, Duration::from_secs(30))
                .expect("duplicate"),
            ReceiveDisposition::Redundant
        );
        let never_requested = BlockKey::new(0, BLOCK * 4, BLOCK).expect("valid shape");
        assert_eq!(
            state
                .receive_block(connection(1), never_requested, Duration::from_secs(30))
                .expect("unsolicited classification"),
            ReceiveDisposition::Unsolicited
        );
        assert_eq!(
            state.mark_piece_verified(0).expect("verify late source"),
            vec![connection(1)]
        );
    }

    #[test]
    fn strict_endgame_duplicates_one_idle_peer_and_first_response_cancels_loser() {
        let mut state = state(1, vec![plan(0, 2)], 3);
        for id in 1..=3 {
            add_peer(&mut state, connection(id), &[0], false);
        }

        let assigned = state.schedule(Duration::ZERO).expect("endgame schedule");
        assert_eq!(assigned.len(), 3);
        let duplicate_block = assigned
            .iter()
            .find_map(|candidate| {
                (assigned
                    .iter()
                    .filter(|request| request.block == candidate.block)
                    .count()
                    == 2)
                    .then_some(candidate.block)
            })
            .expect("one duplicated block");
        let owners = assigned
            .iter()
            .filter(|request| request.block == duplicate_block)
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 2);
        let initial = state.snapshot(Duration::ZERO);
        assert_eq!(initial.requested_blocks, 2);
        assert_eq!(initial.active_request_attempts, 3);
        assert_eq!(initial.active_duplicate_attempts, 1);
        assert_eq!(initial.endgame_assignments, 1);
        assert_eq!(initial.outstanding_request_bytes, 3 * BLOCK as usize);
        assert_cached_indexes(&state);

        let winner = owners[1];
        let loser = owners[0];
        assert_eq!(
            state
                .receive_block(winner.connection, winner.block, Duration::from_millis(1))
                .expect("winning block"),
            ReceiveDisposition::Accept {
                evidence: winner.attempt,
                cancellations: vec![RequestCancellation {
                    attempt: loser.attempt,
                    connection: loser.connection,
                    block: loser.block,
                }],
                late: false,
            }
        );
        let writing = state.snapshot(Duration::from_millis(1));
        assert_eq!(writing.active_request_attempts, 1);
        assert_eq!(writing.active_duplicate_attempts, 0);
        assert_eq!(writing.cancelled_request_attempts, 1);
        assert_eq!(writing.outstanding_request_bytes, BLOCK as usize);
        assert_cached_indexes(&state);
        state
            .finish_write(winner.block, true, Duration::from_millis(1))
            .expect("winning write");
        assert_cached_indexes(&state);
        assert_eq!(
            state
                .receive_block(loser.connection, loser.block, Duration::from_millis(2))
                .expect("late losing payload"),
            ReceiveDisposition::Redundant
        );
        assert_eq!(
            state
                .snapshot(Duration::from_millis(2))
                .redundant_payload_bytes,
            BLOCK as usize
        );
    }

    #[test]
    fn strict_endgame_waits_until_every_ordinary_block_is_covered() {
        let mut state = state(2, vec![plan(0, 1), plan(1, 1)], 3);
        for id in 1..=3 {
            add_peer(&mut state, connection(id), &[0], false);
        }

        let assigned = state.schedule(Duration::ZERO).expect("ordinary schedule");

        assert_eq!(assigned.len(), 1);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.missing_blocks, 1);
        assert_eq!(snapshot.active_request_attempts, 1);
        assert_eq!(snapshot.active_duplicate_attempts, 0);
        assert_eq!(snapshot.endgame_assignments, 0);
    }

    #[test]
    fn terminating_one_endgame_owner_keeps_the_other_request_live() {
        let mut state = state(1, vec![plan(0, 1)], 2);
        add_peer(&mut state, connection(1), &[0], false);
        add_peer(&mut state, connection(2), &[0], false);
        let assigned = state.schedule(Duration::ZERO).expect("endgame schedule");
        assert_eq!(assigned.len(), 2);

        let released = state
            .remove_connection(connection(1), ConnectionRemoval::Disconnected)
            .expect("disconnect one owner");

        assert_eq!(released, vec![assigned[0].block]);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.requested_blocks, 1);
        assert_eq!(snapshot.active_request_attempts, 1);
        assert_eq!(snapshot.active_duplicate_attempts, 0);
        assert_eq!(snapshot.outstanding_request_bytes, BLOCK as usize);
        assert_eq!(
            state.block_status(assigned[0].block).expect("block status"),
            BlockStatus::Requested
        );
        state
            .remove_connection(connection(2), ConnectionRemoval::Disconnected)
            .expect("disconnect final owner");
        assert_eq!(
            state
                .block_status(assigned[0].block)
                .expect("missing block"),
            BlockStatus::Missing
        );
        assert_eq!(state.snapshot(Duration::ZERO).outstanding_request_bytes, 0);
        add_peer(&mut state, connection(3), &[0], false);
        assert_eq!(
            state
                .schedule(Duration::from_secs(1))
                .expect("reschedule")
                .len(),
            1
        );
    }

    #[test]
    fn disconnect_releases_requests_but_not_torrent_owned_writes() {
        let mut state = state(1, vec![plan(0, 2)], 2);
        add_peer(&mut state, connection(1), &[0], false);
        let assigned = state.schedule(Duration::ZERO).expect("schedule");
        state
            .receive_block(connection(1), assigned[0].block, Duration::ZERO)
            .expect("payload");
        let released = state
            .remove_connection(connection(1), ConnectionRemoval::Disconnected)
            .expect("disconnect");
        assert_eq!(released, vec![assigned[1].block]);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.writing_blocks, 1);
        assert_eq!(snapshot.outstanding_request_bytes, 0);
        state
            .finish_write(assigned[0].block, true, Duration::ZERO)
            .expect("write");
        assert_eq!(state.snapshot(Duration::ZERO).outstanding_request_bytes, 0);
    }

    #[test]
    fn split_availability_completes_through_two_peers() {
        let mut state = state(2, vec![plan(0, 1), plan(1, 1)], 2);
        add_peer(&mut state, connection(1), &[0], false);
        add_peer(&mut state, connection(2), &[1], false);
        let assigned = state.schedule(Duration::ZERO).expect("schedule");
        assert_eq!(assigned.len(), 2);
        for request in assigned {
            assert!(matches!(
                state
                    .receive_block(request.connection, request.block, Duration::ZERO)
                    .expect("payload"),
                ReceiveDisposition::Accept { .. }
            ));
            state
                .finish_write(request.block, true, Duration::ZERO)
                .expect("write");
        }
        for piece in 0..2 {
            assert!(state.piece_ready(piece).expect("ready"));
            state.mark_piece_verified(piece).expect("verify");
        }
        assert_eq!(
            state.snapshot(Duration::ZERO).no_request_reason,
            Some(NoRequestReason::Complete)
        );
    }

    #[test]
    fn hash_failure_resets_only_one_piece_and_retains_bounded_contributors() {
        let mut state = state(2, vec![plan(0, 2), plan(1, 1)], 1);
        add_peer(&mut state, connection(3), &[1], false);
        let unrelated = state.schedule(Duration::ZERO).expect("unrelated piece")[0];
        state
            .receive_block(unrelated.connection, unrelated.block, Duration::ZERO)
            .expect("unrelated payload");
        state
            .finish_write(unrelated.block, true, Duration::ZERO)
            .expect("unrelated write");
        assert_eq!(
            state.mark_piece_verified(1).expect("unrelated verify"),
            vec![connection(3)]
        );
        state.set_choking(connection(3), true).expect("choke peer");

        add_peer(&mut state, connection(1), &[0], false);
        let first = state.schedule(Duration::ZERO).expect("first block")[0];
        state
            .receive_block(first.connection, first.block, Duration::ZERO)
            .expect("first payload");
        state
            .finish_write(first.block, true, Duration::ZERO)
            .expect("first write");
        state.set_choking(connection(1), true).expect("choke first");

        add_peer(&mut state, connection(2), &[0], false);
        let second = state.schedule(Duration::ZERO).expect("second block")[0];
        state
            .receive_block(second.connection, second.block, Duration::ZERO)
            .expect("second payload");
        state
            .finish_write(second.block, true, Duration::ZERO)
            .expect("second write");

        let failure = state.mark_piece_hash_failed(0).expect("hash failure");
        assert_eq!(failure.piece, 0);
        assert_eq!(failure.contributors, vec![connection(1), connection(2)]);
        assert_eq!(failure.failed_bytes, 2 * BLOCK as usize);
        let failed = state.snapshot(Duration::ZERO);
        assert_eq!(failed.missing_blocks, 2);
        assert_eq!(failed.verified_blocks, 1);
        assert_eq!(failed.outstanding_request_bytes, 0);
        assert_eq!(failed.piece_hash_failures, 1);
        assert_eq!(failed.failed_piece_bytes, 2 * BLOCK as usize);
        assert_eq!(failed.last_hash_failure_contributors, 2);
        assert_cached_indexes(&state);

        state
            .set_choking(connection(2), true)
            .expect("choke second");
        add_peer(&mut state, connection(4), &[0], false);
        for _ in 0..2 {
            let request = state.schedule(Duration::ZERO).expect("retry")[0];
            assert_eq!(request.connection, connection(4));
            state
                .receive_block(request.connection, request.block, Duration::ZERO)
                .expect("retry payload");
            state
                .finish_write(request.block, true, Duration::ZERO)
                .expect("retry write");
        }
        assert_eq!(
            state.mark_piece_verified(0).expect("retry verify"),
            vec![connection(4)]
        );
        assert_eq!(
            state.snapshot(Duration::ZERO).no_request_reason,
            Some(NoRequestReason::Complete)
        );
        assert_cached_indexes(&state);
    }

    #[test]
    fn hash_failure_rejects_incomplete_or_already_verified_piece() {
        let mut state = state(1, vec![plan(0, 1)], 1);
        assert!(matches!(
            state.mark_piece_hash_failed(0),
            Err(SwarmError::InvalidTransition(_))
        ));
        add_peer(&mut state, connection(1), &[0], false);
        let request = state.schedule(Duration::ZERO).expect("request")[0];
        state
            .receive_block(request.connection, request.block, Duration::ZERO)
            .expect("payload");
        state
            .finish_write(request.block, true, Duration::ZERO)
            .expect("write");
        state.mark_piece_verified(0).expect("verify");
        assert!(matches!(
            state.mark_piece_hash_failed(0),
            Err(SwarmError::InvalidTransition(_))
        ));
    }

    #[test]
    fn full_choked_set_replaces_only_after_grace_and_protects_unique_data() {
        let mut config = SwarmConfig::for_request_limit(BLOCK as usize);
        config.max_established_connections = 2;
        config.unproductive_grace = Duration::from_secs(30);
        let mut state =
            SwarmState::new(config, 2, vec![plan(0, 1), plan(1, 1)]).expect("swarm state");
        add_peer(&mut state, connection(1), &[0, 1], true);
        add_peer(&mut state, connection(2), &[0], true);
        assert_eq!(
            state.snapshot(Duration::from_secs(29)).next_replacement_at,
            Some(Duration::from_secs(30))
        );
        assert_eq!(state.replacement_candidate(Duration::from_secs(29)), None);
        assert_eq!(
            state.replacement_candidate(Duration::from_secs(30)),
            Some(connection(2))
        );
        assert_eq!(
            state.snapshot(Duration::from_secs(30)).no_request_reason,
            Some(NoRequestReason::AllUsefulPeersChoked)
        );
    }

    #[test]
    fn irrelevant_peer_is_replaced_but_no_capacity_does_not_trigger_churn() {
        let mut config = SwarmConfig::for_request_limit(BLOCK as usize);
        config.max_established_connections = 2;
        config.unproductive_grace = Duration::from_secs(10);
        let mut state = SwarmState::new(config, 1, vec![plan(0, 1)]).expect("swarm");
        add_peer(&mut state, connection(1), &[0], true);
        add_peer(&mut state, connection(2), &[], true);
        assert_eq!(
            state.replacement_candidate(Duration::from_secs(10)),
            Some(connection(2))
        );
        state
            .remove_connection(connection(2), ConnectionRemoval::Replaced)
            .expect("replace");
        assert_eq!(state.replacement_candidate(Duration::from_secs(100)), None);
    }

    #[test]
    fn retained_attempt_history_and_active_piece_count_stay_bounded() {
        let mut config = SwarmConfig::for_request_limit(BLOCK as usize);
        config.max_terminal_attempts_per_block = 2;
        config.max_active_piece_bytes = 2 * BLOCK as usize;
        config.request_timeout = Duration::from_secs(1);
        let mut state = SwarmState::new(config, 2, vec![plan(0, 2), plan(1, 1)]).expect("swarm");
        add_peer(&mut state, connection(1), &[0, 1], false);
        for second in 0..5 {
            state
                .schedule(Duration::from_secs(second))
                .expect("schedule");
            state
                .expire_requests(Duration::from_secs(second + 1))
                .expect("expire");
        }
        let first = BlockKey::new(0, 0, BLOCK).expect("block");
        assert!(state.attempts_for(first).expect("attempts").len() <= 2);
        let assigned = state.schedule(Duration::from_secs(10)).expect("schedule");
        assert!(
            assigned
                .iter()
                .all(|assignment| assignment.block.piece == 0)
        );
    }

    #[test]
    fn availability_accounting_is_exact_through_duplicates_seed_transition_and_disconnect() {
        let mut state = SwarmState::new_with_wanted(
            SwarmConfig::for_request_limit(4 * BLOCK as usize),
            4,
            vec![0, 1, 2, 3],
            Vec::new(),
            0x91,
        )
        .expect("unplanned swarm");
        state
            .add_connection(connection(1), Duration::ZERO)
            .expect("first peer");
        state
            .set_bitfield(connection(1), vec![true, false, true, false])
            .expect("first bitfield");
        state.peer_has(connection(1), 2).expect("duplicate have");
        assert_eq!(state.piece_availability(2), Ok(1));
        state.peer_has(connection(1), 1).expect("new have");
        state.peer_has(connection(1), 3).expect("seed transition");
        assert_eq!(state.picker.seed_count(), 1);
        assert_eq!(
            (0..4)
                .map(|piece| state.piece_availability(piece).expect("availability"))
                .collect::<Vec<_>>(),
            vec![1, 1, 1, 1]
        );

        add_peer(&mut state, connection(2), &[0, 1], false);
        assert_eq!(
            (0..4)
                .map(|piece| state.piece_availability(piece).expect("availability"))
                .collect::<Vec<_>>(),
            vec![2, 2, 1, 1]
        );
        state
            .remove_connection(connection(1), ConnectionRemoval::Disconnected)
            .expect("remove seed");
        assert_eq!(state.picker.seed_count(), 0);
        assert_eq!(
            (0..4)
                .map(|piece| state.piece_availability(piece).expect("availability"))
                .collect::<Vec<_>>(),
            vec![1, 1, 0, 0]
        );
        assert_cached_indexes(&state);
    }

    #[test]
    fn global_rarest_piece_is_planned_and_activated_before_common_work() {
        let mut state = SwarmState::new_with_wanted(
            SwarmConfig::for_request_limit(4 * BLOCK as usize),
            4,
            vec![0, 1, 2, 3],
            Vec::new(),
            0,
        )
        .expect("unplanned swarm");
        add_peer(&mut state, connection(1), &[0, 1, 2], false);
        add_peer(&mut state, connection(2), &[0, 1], false);

        let piece = state
            .reserve_piece_for_planning(256)
            .expect("rarest plan reservation");
        assert_eq!(piece, 2);
        state
            .append_piece_plans(vec![plan(piece, 1)])
            .expect("rarest plan");
        let assignments = state.schedule(Duration::ZERO).expect("schedule");
        assert_eq!(assignments[0].block.piece, 2);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.last_activated_piece, Some(2));
        assert_eq!(snapshot.last_activated_availability, Some(1));

        let mut config = SwarmConfig::for_request_limit(4 * BLOCK as usize);
        config.piece_activation_policy = PieceActivationPolicy::InOrder;
        let mut in_order = SwarmState::new_with_wanted(config, 4, vec![0, 1, 2, 3], Vec::new(), 0)
            .expect("in-order swarm");
        add_peer(&mut in_order, connection(1), &[0, 1, 2], false);
        add_peer(&mut in_order, connection(2), &[0, 1], false);
        assert_eq!(in_order.reserve_piece_for_planning(256), Some(0));
    }

    #[test]
    fn fast_suggestions_precede_only_ordinary_equal_rarity_ties() {
        let mut tied = SwarmState::new_with_wanted(
            SwarmConfig::for_request_limit(4 * BLOCK as usize),
            3,
            vec![0, 1, 2],
            Vec::new(),
            0,
        )
        .expect("unplanned swarm");
        add_peer(&mut tied, connection(1), &[0, 1, 2], false);
        tied.set_fast_extension(connection(1), true)
            .expect("enable Fast");
        let initial = PeerMessage::Bitfield(vec![0b1110_0000]);
        tied.observe_fast_message(connection(1), &initial)
            .expect("initial availability");
        tied.set_compact_bitfield(connection(1), vec![0b1110_0000])
            .expect("apply availability");
        tied.observe_fast_message(connection(1), &PeerMessage::SuggestPiece(1))
            .expect("suggest equal-rarity piece");
        assert_eq!(tied.reserve_piece_for_planning(256), Some(1));

        let mut rarest = SwarmState::new_with_wanted(
            SwarmConfig::for_request_limit(4 * BLOCK as usize),
            3,
            vec![0, 1, 2],
            Vec::new(),
            0,
        )
        .expect("unplanned swarm");
        add_peer(&mut rarest, connection(1), &[0, 1, 2], false);
        add_peer(&mut rarest, connection(2), &[0], false);
        rarest
            .set_fast_extension(connection(1), true)
            .expect("enable Fast");
        rarest
            .observe_fast_message(connection(1), &initial)
            .expect("initial availability");
        rarest
            .set_compact_bitfield(connection(1), vec![0b1110_0000])
            .expect("apply availability");
        rarest
            .observe_fast_message(connection(1), &PeerMessage::SuggestPiece(0))
            .expect("suggest common piece");
        assert_ne!(rarest.reserve_piece_for_planning(256), Some(0));
    }

    #[test]
    fn bounded_planning_sweep_reaches_common_work_behind_blocked_rare_pieces() {
        let mut state = SwarmState::new_with_wanted(
            SwarmConfig::for_request_limit(4 * BLOCK as usize),
            300,
            (0..300).collect(),
            Vec::new(),
            0,
        )
        .expect("unplanned swarm");
        add_peer(
            &mut state,
            connection(1),
            &(0..=256).collect::<Vec<_>>(),
            true,
        );
        add_peer(&mut state, connection(2), &[257], false);
        add_peer(&mut state, connection(3), &[257], false);

        assert_eq!(state.reserve_piece_for_planning(256), None);
        assert_eq!(
            state.snapshot(Duration::ZERO).picker_candidate_inspections,
            256
        );
        assert_eq!(state.reserve_piece_for_planning(256), Some(257));
        assert_eq!(
            state.snapshot(Duration::ZERO).picker_candidate_inspections,
            258
        );
    }

    #[test]
    fn plan_lookahead_is_independent_from_the_current_request_window() {
        let mut state = SwarmState::new_with_wanted(
            SwarmConfig::for_request_limit(2 * BLOCK as usize),
            2,
            vec![0, 1],
            vec![plan(0, 2)],
            0,
        )
        .expect("lookahead swarm");
        add_peer(&mut state, connection(1), &[0, 1], false);
        assert_eq!(state.schedule(Duration::ZERO).expect("schedule").len(), 2);
        assert_eq!(state.connection_request_count(connection(1)), 2);
        state.inactive_planned_piece_visits = 0;
        assert!(
            state
                .schedule(Duration::ZERO)
                .expect("full window")
                .is_empty()
        );
        assert_eq!(state.inactive_planned_piece_visits, 0);
        assert_eq!(state.reserve_piece_for_planning(256), Some(1));
    }

    #[test]
    fn full_request_window_never_visits_the_inactive_lookahead() {
        const PLANNED: u32 = 256;
        const ATTEMPTS: usize = 100_000;
        let plans = (0..PLANNED).map(|piece| plan(piece, 1)).collect();
        let mut state = SwarmState::new(
            SwarmConfig::for_request_limit(2 * BLOCK as usize),
            PLANNED as usize,
            plans,
        )
        .expect("hostile lookahead swarm");
        let pieces = (0..PLANNED as usize).collect::<Vec<_>>();
        add_peer(&mut state, connection(1), &pieces, false);
        assert_eq!(state.schedule(Duration::ZERO).expect("schedule").len(), 2);

        state.inactive_planned_piece_visits = 0;
        for _ in 0..ATTEMPTS {
            assert!(
                state
                    .schedule(Duration::ZERO)
                    .expect("full request window")
                    .is_empty()
            );
        }
        assert_eq!(state.inactive_planned_piece_visits, 0);
    }

    #[test]
    fn unique_unplanned_wanted_piece_protects_its_peer_from_replacement() {
        let mut config = SwarmConfig::for_request_limit(4 * BLOCK as usize);
        config.max_established_connections = 3;
        config.unproductive_grace = Duration::from_secs(1);
        let mut state =
            SwarmState::new_with_wanted(config, 4, vec![0, 1, 2, 3], vec![plan(0, 1)], 0)
                .expect("partially planned swarm");
        add_peer(&mut state, connection(1), &[3], true);
        add_peer(&mut state, connection(2), &[0], true);
        add_peer(&mut state, connection(3), &[0], true);

        assert_eq!(state.connections[&connection(1)].wanted_piece_count, 1);
        assert_eq!(
            state.replacement_candidate(Duration::from_secs(1)),
            Some(connection(2))
        );
    }

    #[test]
    fn active_piece_count_is_independent_from_the_byte_ceiling() {
        let mut config = SwarmConfig::for_request_limit(4 * BLOCK as usize);
        config.initial_requests_per_connection = 4;
        config.max_active_pieces = 2;
        let mut state = SwarmState::new(config, 4, (0..4).map(|piece| plan(piece, 1)).collect())
            .expect("count-bounded swarm");
        add_peer(&mut state, connection(1), &[0, 1, 2, 3], false);
        let assignments = state.schedule(Duration::ZERO).expect("schedule");
        assert_eq!(assignments.len(), 2);
        assert_eq!(state.snapshot(Duration::ZERO).active_piece_count, 2);
    }

    #[test]
    fn partial_pressure_bounds_new_activation_without_cancelling_owned_work() {
        let mut config = SwarmConfig::for_request_limit(64 * 1024 * 1024);
        config.max_active_piece_bytes = 64 * 1024 * 1024;
        let mut peer_bounded =
            SwarmState::new(config, 3, (0..3).map(|piece| plan(piece, 2)).collect())
                .expect("peer-pressure swarm");
        add_peer(&mut peer_bounded, connection(1), &[0, 1, 2], false);
        peer_bounded.activate_piece(0).expect("first partial");
        assert!(peer_bounded.can_activate_piece(1));
        peer_bounded.activate_piece(1).expect("threshold crossing");
        assert!(!peer_bounded.can_activate_piece(2));
        assert_eq!(peer_bounded.active_pieces.len(), 2);

        let mut block_bounded = SwarmState::new(config, 2, vec![plan(0, 2_049), plan(1, 1)])
            .expect("block-pressure swarm");
        add_peer(&mut block_bounded, connection(1), &[0, 1], false);
        block_bounded
            .activate_piece(0)
            .expect("one oversized partial span");
        assert_eq!(block_bounded.requestable_active_blocks, 2_049);
        assert!(!block_bounded.can_activate_piece(1));
        assert_eq!(block_bounded.active_pieces.len(), 1);
    }

    #[test]
    fn falling_peer_count_stops_new_activation_but_retains_active_pieces() {
        let mut config = SwarmConfig::for_request_limit(64 * BLOCK as usize);
        config.max_active_piece_bytes = 64 * BLOCK as usize;
        let mut state = SwarmState::new(config, 7, (0..7).map(|piece| plan(piece, 2)).collect())
            .expect("peer-pressure swarm");
        for id in 1..=3 {
            add_peer(
                &mut state,
                connection(id),
                &(0..7).collect::<Vec<_>>(),
                false,
            );
        }
        for piece in 0..5 {
            assert!(state.can_activate_piece(piece));
            state.activate_piece(piece as u32).expect("active piece");
        }
        state
            .remove_connection(connection(2), ConnectionRemoval::Disconnected)
            .expect("second peer leaves");
        state
            .remove_connection(connection(3), ConnectionRemoval::Disconnected)
            .expect("third peer leaves");
        assert!(!state.can_activate_piece(5));
        assert_eq!(state.active_pieces.len(), 5);
    }

    #[test]
    fn maximum_active_hot_path_does_not_touch_the_global_rarity_index() {
        const ACTIVE: usize = DEFAULT_MAX_ACTIVE_PIECES;
        let mut config = SwarmConfig::for_request_limit(ACTIVE * BLOCK as usize);
        config.max_active_piece_bytes = ACTIVE * BLOCK as usize;
        let mut state = SwarmState::new(
            config,
            ACTIVE,
            (0..ACTIVE as u32).map(|piece| plan(piece, 1)).collect(),
        )
        .expect("maximum active swarm");
        add_peer(&mut state, connection(1), &[ACTIVE - 1], false);
        for piece in 0..ACTIVE as u32 {
            state.activate_piece(piece).expect("activate piece");
        }
        state.picker.reset_counters();
        state.active_piece_visits = 0;
        let started = std::time::Instant::now();
        for _ in 0..100_000 {
            assert!(
                state
                    .next_active_block_for_connection(connection(1))
                    .expect("active selection")
                    .is_some()
            );
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        let counters = state.picker.counters();
        assert_eq!(counters.rank_comparisons, 0);
        assert_eq!(counters.single_piece_updates, 0);
        assert_eq!(counters.bulk_rebuilds, 0);
        assert_eq!(counters.candidate_inspections, 0);
        assert_eq!(state.active_piece_visits, 100_000 * ACTIVE as u64);
        assert_eq!(state.inactive_planned_piece_visits, 0);
        assert_eq!(state.active_pieces.len(), ACTIVE);
    }

    #[test]
    #[ignore = "manual release-mode active picker comparison"]
    fn active_hot_path_policy_timing_profile() {
        const ACTIVE: usize = DEFAULT_MAX_ACTIVE_PIECES;
        const ATTEMPTS: usize = 20_000;

        fn profile_state(policy: PieceActivationPolicy) -> SwarmState {
            let mut config = SwarmConfig::for_request_limit(ACTIVE * BLOCK as usize);
            config.max_active_piece_bytes = ACTIVE * BLOCK as usize;
            config.piece_activation_policy = policy;
            let mut state = SwarmState::new(
                config,
                ACTIVE,
                (0..ACTIVE as u32).map(|piece| plan(piece, 1)).collect(),
            )
            .expect("profile swarm");
            add_peer(&mut state, connection(1), &[ACTIVE - 1], false);
            for piece in 0..ACTIVE as u32 {
                state.activate_piece(piece).expect("activate piece");
            }
            state
        }

        fn sample(state: &mut SwarmState) -> Duration {
            let started = std::time::Instant::now();
            for _ in 0..ATTEMPTS {
                assert!(
                    state
                        .next_active_block_for_connection(connection(1))
                        .expect("active selection")
                        .is_some()
                );
            }
            started.elapsed()
        }

        let mut in_order = profile_state(PieceActivationPolicy::InOrder);
        let mut rarest = profile_state(PieceActivationPolicy::RarestFirst);
        let _ = sample(&mut in_order);
        let _ = sample(&mut rarest);
        let mut in_order_samples = Vec::new();
        let mut rarest_samples = Vec::new();
        for _ in 0..5 {
            in_order_samples.push(sample(&mut in_order));
            rarest_samples.push(sample(&mut rarest));
        }
        in_order_samples.sort_unstable();
        rarest_samples.sort_unstable();
        let in_order_median = in_order_samples[2];
        let rarest_median = rarest_samples[2];
        let in_order_p95 = in_order_samples[4];
        let rarest_p95 = rarest_samples[4];
        println!(
            "active={ACTIVE} attempts_per_sample={ATTEMPTS} in_order_median_us={} rarest_median_us={} median_ratio={:.3} in_order_p95_us={} rarest_p95_us={} p95_ratio={:.3}",
            in_order_median.as_micros(),
            rarest_median.as_micros(),
            rarest_median.as_secs_f64() / in_order_median.as_secs_f64(),
            in_order_p95.as_micros(),
            rarest_p95.as_micros(),
            rarest_p95.as_secs_f64() / in_order_p95.as_secs_f64(),
        );
        assert!(rarest_median.as_secs_f64() <= in_order_median.as_secs_f64() * 1.10);
        assert!(rarest_p95.as_secs_f64() <= in_order_p95.as_secs_f64() * 1.20);
    }

    #[test]
    fn maximum_peer_availability_is_retained_as_a_compact_bitfield() {
        let piece_count = rstorrent_protocol::metainfo::MAX_METAINFO_PIECES;
        let mut state = SwarmState::new(
            SwarmConfig::for_request_limit(BLOCK as usize),
            piece_count,
            vec![plan(0, 1)],
        )
        .expect("maximum geometry swarm");
        state
            .add_connection(connection(1), Duration::ZERO)
            .expect("maximum geometry peer");
        let mut bitfield = vec![0; piece_count.div_ceil(8)];
        bitfield[0] = 0x80;
        let last_byte = bitfield.len() - 1;
        bitfield[last_byte] = 0x01;
        state
            .set_compact_bitfield(connection(1), bitfield)
            .expect("compact maximum bitfield");

        let availability = &state
            .connections
            .get(&connection(1))
            .expect("retained peer")
            .availability;
        assert_eq!(availability.bytes.len(), 262_144);
        assert!(availability.contains(0));
        assert!(availability.contains(piece_count - 1));
    }

    #[test]
    fn cancellation_releases_every_reservation_and_pending_dial() {
        let mut state = state(1, vec![plan(0, 2)], 2);
        state.begin_dial(dial(1)).expect("dial");
        add_peer(&mut state, connection(1), &[0], false);
        let assigned = state.schedule(Duration::ZERO).expect("schedule");
        state
            .receive_block(connection(1), assigned[0].block, Duration::ZERO)
            .expect("payload");
        state.cancel_all().expect("cancel");
        assert_cached_indexes(&state);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.pending_dials, 0);
        assert_eq!(snapshot.outstanding_request_bytes, 0);
        assert_eq!(snapshot.requested_blocks, 0);
        assert_eq!(snapshot.writing_blocks, 0);
    }
}

//! Runtime-independent multi-peer request ownership and scheduling.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::time::Duration;

use rstorrent_protocol::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH};
use rstorrent_protocol::piece::MIN_PAYLOAD_ALLOWANCE;

pub const DEFAULT_MAX_ESTABLISHED_CONNECTIONS: usize = 8;
pub const DEFAULT_MAX_PENDING_DIALS: usize = 3;
pub const DEFAULT_MAX_REQUESTS_PER_CONNECTION: usize = 4;
pub const DEFAULT_MAX_ACTIVE_PIECES: usize = 4;
pub const DEFAULT_MAX_TERMINAL_ATTEMPTS_PER_BLOCK: usize = 4;
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_UNPRODUCTIVE_GRACE: Duration = Duration::from_secs(60);

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
    pub max_requests_per_connection: usize,
    pub max_active_pieces: usize,
    pub max_terminal_attempts_per_block: usize,
    pub payload_limit: usize,
    pub request_timeout: Duration,
    pub unproductive_grace: Duration,
}

impl SwarmConfig {
    pub const fn for_payload_limit(payload_limit: usize) -> Self {
        Self {
            max_established_connections: DEFAULT_MAX_ESTABLISHED_CONNECTIONS,
            max_pending_dials: DEFAULT_MAX_PENDING_DIALS,
            max_requests_per_connection: DEFAULT_MAX_REQUESTS_PER_CONNECTION,
            max_active_pieces: DEFAULT_MAX_ACTIVE_PIECES,
            max_terminal_attempts_per_block: DEFAULT_MAX_TERMINAL_ATTEMPTS_PER_BLOCK,
            payload_limit,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
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
        if self.max_requests_per_connection == 0 {
            return Err(SwarmError::InvalidConfig(
                "per-connection request limit must be nonzero",
            ));
        }
        if self.max_active_pieces == 0 {
            return Err(SwarmError::InvalidConfig(
                "active piece limit must be nonzero",
            ));
        }
        if self.max_terminal_attempts_per_block == 0 {
            return Err(SwarmError::InvalidConfig(
                "terminal attempt retention must be nonzero",
            ));
        }
        if self.payload_limit < MIN_PAYLOAD_ALLOWANCE {
            return Err(SwarmError::InvalidConfig(
                "payload limit is smaller than one request block",
            ));
        }
        if self.request_timeout.is_zero() || self.unproductive_grace.is_zero() {
            return Err(SwarmError::InvalidConfig(
                "request and unproductive timeouts must be nonzero",
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
pub enum ReceiveDisposition {
    Accept {
        evidence: RequestAttemptId,
        superseded: Option<RequestAttemptId>,
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
    PayloadAllowanceFull,
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
    pub writing_blocks: usize,
    pub received_blocks: usize,
    pub verified_blocks: usize,
    pub payload_reserved: usize,
    pub payload_high_water: usize,
    pub oldest_request_age: Option<Duration>,
    pub next_request_expiry: Option<Duration>,
    pub no_request_reason: Option<NoRequestReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionRemoval {
    Disconnected,
    Replaced,
    Cancelled,
}

#[derive(Debug)]
struct ConnectionState {
    availability: Vec<bool>,
    choking: bool,
    connected_at: Duration,
    last_useful_at: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockPhase {
    Missing,
    Requested(RequestAttemptId),
    Writing {
        source: ConnectionId,
        evidence: RequestAttemptId,
    },
    Received,
    Verified,
}

impl BlockPhase {
    const fn status(self) -> BlockStatus {
        match self {
            Self::Missing => BlockStatus::Missing,
            Self::Requested(_) => BlockStatus::Requested,
            Self::Writing { .. } => BlockStatus::Writing,
            Self::Received => BlockStatus::Received,
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
}

#[derive(Debug)]
pub struct SwarmState {
    config: SwarmConfig,
    piece_count: usize,
    pieces: BTreeMap<u32, PieceState>,
    blocks: BTreeMap<BlockKey, BlockState>,
    connections: BTreeMap<ConnectionId, ConnectionState>,
    pending_dials: BTreeSet<PendingDialId>,
    next_attempt_id: u64,
    payload_reserved: usize,
    payload_high_water: usize,
    last_scheduled_connection: Option<ConnectionId>,
}

impl SwarmState {
    pub fn new(
        config: SwarmConfig,
        piece_count: usize,
        plans: Vec<PiecePlan>,
    ) -> Result<Self, SwarmError> {
        let config = config.validate()?;
        if piece_count == 0 {
            return Err(SwarmError::InvalidConfig("piece count must be nonzero"));
        }
        if plans.is_empty() {
            return Err(SwarmError::InvalidConfig(
                "at least one wanted piece plan is required",
            ));
        }
        let mut pieces = BTreeMap::new();
        let mut blocks = BTreeMap::new();
        for plan in plans {
            if usize::try_from(plan.index).map_or(true, |index| index >= piece_count) {
                return Err(SwarmError::PieceOutOfRange {
                    piece: plan.index,
                    piece_count,
                });
            }
            if pieces.contains_key(&plan.index) {
                return Err(SwarmError::DuplicatePiecePlan(plan.index));
            }
            for block in &plan.blocks {
                if blocks
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
            pieces.insert(
                plan.index,
                PieceState {
                    blocks: plan.blocks,
                },
            );
        }
        Ok(Self {
            config,
            piece_count,
            pieces,
            blocks,
            connections: BTreeMap::new(),
            pending_dials: BTreeSet::new(),
            next_attempt_id: 1,
            payload_reserved: 0,
            payload_high_water: 0,
            last_scheduled_connection: None,
        })
    }

    pub const fn config(&self) -> SwarmConfig {
        self.config
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
                availability: vec![false; self.piece_count],
                choking: true,
                connected_at: now,
                last_useful_at: None,
            },
        );
        Ok(())
    }

    pub fn remove_connection(
        &mut self,
        id: ConnectionId,
        removal: ConnectionRemoval,
    ) -> Result<Vec<BlockKey>, SwarmError> {
        if self.connections.remove(&id).is_none() {
            return Err(SwarmError::UnknownConnection(id));
        }
        let disposition = match removal {
            ConnectionRemoval::Disconnected | ConnectionRemoval::Replaced => {
                RequestDisposition::Disconnected
            }
            ConnectionRemoval::Cancelled => RequestDisposition::Cancelled,
        };
        self.release_requests_for_connection(id, disposition)
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
        self.connection_mut(id)?.availability = availability;
        Ok(())
    }

    pub fn peer_has(&mut self, id: ConnectionId, piece: u32) -> Result<(), SwarmError> {
        let piece_count = self.piece_count;
        let index = usize::try_from(piece)
            .map_err(|_| SwarmError::PieceOutOfRange { piece, piece_count })?;
        if index >= piece_count {
            return Err(SwarmError::PieceOutOfRange { piece, piece_count });
        }
        self.connection_mut(id)?.availability[index] = true;
        Ok(())
    }

    pub fn set_choking(
        &mut self,
        id: ConnectionId,
        choking: bool,
    ) -> Result<Vec<BlockKey>, SwarmError> {
        self.connection_mut(id)?.choking = choking;
        if choking {
            self.release_requests_for_connection(id, RequestDisposition::Choked)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn schedule(&mut self, now: Duration) -> Result<Vec<RequestAssignment>, SwarmError> {
        let mut assignments = Vec::new();
        loop {
            let mut progress = false;
            let ordered = self.ordered_connection_ids();
            for connection in ordered {
                if self.connection_request_count(connection)
                    >= self.config.max_requests_per_connection
                {
                    continue;
                }
                let Some(block) = self.next_block_for_connection(connection)? else {
                    continue;
                };
                let length = usize::try_from(block.length)
                    .map_err(|_| SwarmError::ArithmeticOverflow("block length"))?;
                if self
                    .payload_reserved
                    .checked_add(length)
                    .is_none_or(|reserved| reserved > self.config.payload_limit)
                {
                    continue;
                }
                let assignment = self.assign(connection, block, now)?;
                assignments.push(assignment);
                self.last_scheduled_connection = Some(connection);
                progress = true;
            }
            if !progress {
                break;
            }
        }
        Ok(assignments)
    }

    pub fn expire_requests(&mut self, now: Duration) -> Result<Vec<ExpiredRequest>, SwarmError> {
        let mut expired = Vec::new();
        let active = self
            .blocks
            .iter()
            .filter_map(|(key, block)| match block.phase {
                BlockPhase::Requested(attempt_id) => block
                    .attempts
                    .iter()
                    .find(|attempt| attempt.id == attempt_id)
                    .filter(|attempt| {
                        now.saturating_sub(attempt.issued_at) >= self.config.request_timeout
                    })
                    .map(|attempt| (*key, *attempt)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (block, attempt) in active {
            self.terminate_requested(block, attempt.id, RequestDisposition::Expired)?;
            expired.push(ExpiredRequest {
                attempt: attempt.id,
                connection: attempt.connection,
                block,
            });
        }
        Ok(expired)
    }

    pub fn receive_block(
        &mut self,
        connection: ConnectionId,
        block: BlockKey,
    ) -> Result<ReceiveDisposition, SwarmError> {
        if !self.connections.contains_key(&connection) {
            return Err(SwarmError::UnknownConnection(connection));
        }
        let state = self
            .blocks
            .get(&block)
            .ok_or(SwarmError::UnknownBlock(block))?;
        let evidence = state
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.connection == connection)
            .map(|attempt| attempt.id);
        let BlockPhase::Requested(active_id) = state.phase else {
            return Ok(if evidence.is_some() {
                ReceiveDisposition::Redundant
            } else {
                ReceiveDisposition::Unsolicited
            });
        };
        let Some(evidence_id) = evidence else {
            return Ok(ReceiveDisposition::Unsolicited);
        };
        let active_connection = state
            .attempts
            .iter()
            .find(|attempt| attempt.id == active_id)
            .ok_or(SwarmError::Invariant("active request attempt is missing"))?
            .connection;
        let late = active_connection != connection;
        let superseded = late.then_some(active_id);
        {
            let state = self
                .blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?;
            if let Some(attempt) = state
                .attempts
                .iter_mut()
                .find(|attempt| attempt.id == active_id)
            {
                attempt.disposition = if late {
                    RequestDisposition::Superseded
                } else {
                    RequestDisposition::PayloadReceived
                };
            }
            state.phase = BlockPhase::Writing {
                source: connection,
                evidence: evidence_id,
            };
        }
        Ok(ReceiveDisposition::Accept {
            evidence: evidence_id,
            superseded,
            late,
        })
    }

    pub fn finish_write(
        &mut self,
        block: BlockKey,
        accepted: bool,
        now: Duration,
    ) -> Result<(), SwarmError> {
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
        self.release_payload(block.length)?;
        self.blocks
            .get_mut(&block)
            .ok_or(SwarmError::UnknownBlock(block))?
            .phase = if accepted {
            BlockPhase::Received
        } else {
            BlockPhase::Missing
        };
        if accepted && let Some(connection) = self.connections.get_mut(&source) {
            connection.last_useful_at = Some(now);
        }
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
        let piece = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?;
        Ok(piece.blocks.iter().all(|block| {
            self.blocks.get(block).is_some_and(|block| {
                matches!(block.phase, BlockPhase::Received | BlockPhase::Verified)
            })
        }))
    }

    pub fn mark_piece_verified(&mut self, piece: u32) -> Result<(), SwarmError> {
        if !self.piece_ready(piece)? {
            return Err(SwarmError::InvalidTransition(
                "piece cannot verify before every block is stored",
            ));
        }
        let blocks = self
            .pieces
            .get(&piece)
            .ok_or(SwarmError::UnknownPiece(piece))?
            .blocks
            .clone();
        for block in blocks {
            self.blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?
                .phase = BlockPhase::Verified;
        }
        Ok(())
    }

    pub fn cancel_all(&mut self) -> Result<(), SwarmError> {
        let requested = self
            .blocks
            .iter()
            .filter_map(|(block, state)| match state.phase {
                BlockPhase::Requested(attempt) => Some((*block, attempt)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (block, attempt) in requested {
            self.terminate_requested(block, attempt, RequestDisposition::Cancelled)?;
        }
        let writing = self
            .blocks
            .iter()
            .filter_map(|(block, state)| {
                matches!(state.phase, BlockPhase::Writing { .. }).then_some(*block)
            })
            .collect::<Vec<_>>();
        for block in writing {
            self.release_payload(block.length)?;
            self.blocks
                .get_mut(&block)
                .ok_or(SwarmError::UnknownBlock(block))?
                .phase = BlockPhase::Missing;
        }
        self.pending_dials.clear();
        Ok(())
    }

    pub fn replacement_candidate(&self, now: Duration) -> Option<ConnectionId> {
        if self.connections.len() < self.config.max_established_connections {
            return None;
        }
        let wanted_pieces = self.incomplete_piece_indices();
        self.connections
            .iter()
            .filter(|(id, connection)| {
                now.saturating_sub(connection.last_useful_at.unwrap_or(connection.connected_at))
                    >= self.config.unproductive_grace
                    && !self.has_unique_wanted_piece(**id, &wanted_pieces)
            })
            .filter_map(|(id, connection)| {
                let has_wanted = wanted_pieces
                    .iter()
                    .any(|piece| connection.availability[*piece]);
                let priority = if !has_wanted {
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
        let mut missing = 0;
        let mut requested = 0;
        let mut writing = 0;
        let mut received = 0;
        let mut verified = 0;
        let mut oldest_issued = None;
        let mut next_expiry = None;
        for block in self.blocks.values() {
            match block.phase {
                BlockPhase::Missing => missing += 1,
                BlockPhase::Requested(attempt_id) => {
                    requested += 1;
                    if let Some(attempt) = block
                        .attempts
                        .iter()
                        .find(|attempt| attempt.id == attempt_id)
                    {
                        oldest_issued =
                            Some(oldest_issued.map_or(attempt.issued_at, |oldest: Duration| {
                                oldest.min(attempt.issued_at)
                            }));
                        let deadline = attempt
                            .issued_at
                            .checked_add(self.config.request_timeout)
                            .unwrap_or(Duration::MAX);
                        next_expiry =
                            Some(next_expiry.map_or(deadline, |next: Duration| next.min(deadline)));
                    }
                }
                BlockPhase::Writing { .. } => writing += 1,
                BlockPhase::Received => received += 1,
                BlockPhase::Verified => verified += 1,
            }
        }
        SwarmSnapshot {
            pending_dials: self.pending_dials.len(),
            connected_peers: self.connections.len(),
            unchoked_peers: self
                .connections
                .values()
                .filter(|connection| !connection.choking)
                .count(),
            missing_blocks: missing,
            requested_blocks: requested,
            writing_blocks: writing,
            received_blocks: received,
            verified_blocks: verified,
            payload_reserved: self.payload_reserved,
            payload_high_water: self.payload_high_water,
            oldest_request_age: oldest_issued.map(|issued| now.saturating_sub(issued)),
            next_request_expiry: next_expiry,
            no_request_reason: self.no_request_reason(),
        }
    }

    fn connection_mut(&mut self, id: ConnectionId) -> Result<&mut ConnectionState, SwarmError> {
        self.connections
            .get_mut(&id)
            .ok_or(SwarmError::UnknownConnection(id))
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

    fn next_block_for_connection(
        &self,
        connection: ConnectionId,
    ) -> Result<Option<BlockKey>, SwarmError> {
        let connection = self
            .connections
            .get(&connection)
            .ok_or(SwarmError::UnknownConnection(connection))?;
        if connection.choking {
            return Ok(None);
        }
        let active = self.active_piece_indices();
        for piece in &active {
            if connection.availability[*piece]
                && let Some(block) = self.first_missing_block(*piece as u32)
            {
                return Ok(Some(block));
            }
        }
        if active.len() >= self.config.max_active_pieces {
            return Ok(None);
        }
        for (&piece, state) in &self.pieces {
            let index = usize::try_from(piece)
                .map_err(|_| SwarmError::ArithmeticOverflow("piece index"))?;
            if active.contains(&index)
                || !connection.availability[index]
                || state.blocks.iter().all(|block| {
                    self.blocks
                        .get(block)
                        .is_some_and(|block| matches!(block.phase, BlockPhase::Verified))
                })
            {
                continue;
            }
            if let Some(block) = self.first_missing_block(piece) {
                return Ok(Some(block));
            }
        }
        Ok(None)
    }

    fn first_missing_block(&self, piece: u32) -> Option<BlockKey> {
        self.pieces
            .get(&piece)?
            .blocks
            .iter()
            .copied()
            .find(|block| {
                self.blocks
                    .get(block)
                    .is_some_and(|block| matches!(block.phase, BlockPhase::Missing))
            })
    }

    fn active_piece_indices(&self) -> BTreeSet<usize> {
        self.pieces
            .iter()
            .filter_map(|(&piece, state)| {
                state
                    .blocks
                    .iter()
                    .any(|block| {
                        self.blocks.get(block).is_some_and(|block| {
                            matches!(
                                block.phase,
                                BlockPhase::Requested(_)
                                    | BlockPhase::Writing { .. }
                                    | BlockPhase::Received
                            )
                        })
                    })
                    .then(|| usize::try_from(piece).ok())
                    .flatten()
            })
            .collect()
    }

    fn assign(
        &mut self,
        connection: ConnectionId,
        block: BlockKey,
        now: Duration,
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
        if !matches!(state.phase, BlockPhase::Missing) {
            return Err(SwarmError::InvalidTransition("block is not missing"));
        }
        while state
            .attempts
            .iter()
            .filter(|attempt| !attempt.disposition.is_active())
            .count()
            >= self.config.max_terminal_attempts_per_block
        {
            let Some(index) = state
                .attempts
                .iter()
                .position(|attempt| !attempt.disposition.is_active())
            else {
                return Err(SwarmError::Invariant(
                    "terminal attempt count cannot be reduced",
                ));
            };
            state.attempts.remove(index);
        }
        state.attempts.push_back(RequestAttempt {
            id: attempt_id,
            block,
            connection,
            issued_at: now,
            disposition: RequestDisposition::Requested,
        });
        state.phase = BlockPhase::Requested(attempt_id);
        let length = usize::try_from(block.length)
            .map_err(|_| SwarmError::ArithmeticOverflow("block length"))?;
        self.payload_reserved = self
            .payload_reserved
            .checked_add(length)
            .ok_or(SwarmError::ArithmeticOverflow("payload reservation"))?;
        self.payload_high_water = self.payload_high_water.max(self.payload_reserved);
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
        let state = self
            .blocks
            .get_mut(&block)
            .ok_or(SwarmError::UnknownBlock(block))?;
        if state.phase != BlockPhase::Requested(attempt_id) {
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
        attempt.disposition = disposition;
        state.phase = BlockPhase::Missing;
        self.release_payload(block.length)
    }

    fn release_requests_for_connection(
        &mut self,
        connection: ConnectionId,
        disposition: RequestDisposition,
    ) -> Result<Vec<BlockKey>, SwarmError> {
        let requested = self
            .blocks
            .iter()
            .filter_map(|(block, state)| match state.phase {
                BlockPhase::Requested(attempt_id) => state
                    .attempts
                    .iter()
                    .find(|attempt| attempt.id == attempt_id)
                    .filter(|attempt| attempt.connection == connection)
                    .map(|_| (*block, attempt_id)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut released = Vec::with_capacity(requested.len());
        for (block, attempt) in requested {
            self.terminate_requested(block, attempt, disposition)?;
            released.push(block);
        }
        Ok(released)
    }

    fn release_payload(&mut self, length: u32) -> Result<(), SwarmError> {
        let length =
            usize::try_from(length).map_err(|_| SwarmError::ArithmeticOverflow("block length"))?;
        self.payload_reserved = self
            .payload_reserved
            .checked_sub(length)
            .ok_or(SwarmError::Invariant("payload reservation underflow"))?;
        Ok(())
    }

    fn connection_request_count(&self, connection: ConnectionId) -> usize {
        self.blocks
            .values()
            .filter(|state| match state.phase {
                BlockPhase::Requested(attempt_id) => state
                    .attempts
                    .iter()
                    .any(|attempt| attempt.id == attempt_id && attempt.connection == connection),
                _ => false,
            })
            .count()
    }

    fn incomplete_piece_indices(&self) -> BTreeSet<usize> {
        self.pieces
            .iter()
            .filter_map(|(&piece, state)| {
                state
                    .blocks
                    .iter()
                    .any(|block| {
                        self.blocks
                            .get(block)
                            .is_some_and(|block| !matches!(block.phase, BlockPhase::Verified))
                    })
                    .then(|| usize::try_from(piece).ok())
                    .flatten()
            })
            .collect()
    }

    fn has_unique_wanted_piece(
        &self,
        connection: ConnectionId,
        wanted_pieces: &BTreeSet<usize>,
    ) -> bool {
        let Some(current) = self.connections.get(&connection) else {
            return false;
        };
        wanted_pieces.iter().any(|piece| {
            current.availability[*piece]
                && self
                    .connections
                    .iter()
                    .filter(|(id, state)| **id != connection && state.availability[*piece])
                    .count()
                    == 0
        })
    }

    fn no_request_reason(&self) -> Option<NoRequestReason> {
        if self
            .blocks
            .values()
            .all(|block| matches!(block.phase, BlockPhase::Verified))
        {
            return Some(NoRequestReason::Complete);
        }
        if self.connections.is_empty() {
            return Some(NoRequestReason::NoConnections);
        }
        let wanted = self.incomplete_piece_indices();
        let mut useful = self
            .connections
            .values()
            .filter(|connection| wanted.iter().any(|piece| connection.availability[*piece]));
        let useful_count = useful.clone().count();
        if useful_count == 0 {
            return Some(NoRequestReason::NoPeerHasWantedPiece);
        }
        if useful.all(|connection| connection.choking) {
            return Some(NoRequestReason::AllUsefulPeersChoked);
        }
        if self.payload_reserved >= self.config.payload_limit {
            return Some(NoRequestReason::PayloadAllowanceFull);
        }
        if self.connections.keys().all(|connection| {
            self.connection_request_count(*connection) >= self.config.max_requests_per_connection
        }) {
            return Some(NoRequestReason::RequestWindowsFull);
        }
        if self.active_piece_indices().len() >= self.config.max_active_pieces {
            return Some(NoRequestReason::ActivePieceLimit);
        }
        None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwarmError {
    InvalidConfig(&'static str),
    InvalidBlock { piece: u32, begin: u32, length: u32 },
    EmptyPiecePlan(u32),
    OverlappingBlocks(u32),
    PieceOutOfRange { piece: u32, piece_count: usize },
    DuplicatePiecePlan(u32),
    DuplicateBlock(BlockKey),
    UnknownPiece(u32),
    UnknownBlock(BlockKey),
    DuplicatePendingDial(PendingDialId),
    UnknownPendingDial(PendingDialId),
    PendingDialCapacity,
    DuplicateConnection(ConnectionId),
    UnknownConnection(ConnectionId),
    ConnectionCapacity,
    InvalidAvailability { actual: usize, expected: usize },
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
        let mut config = SwarmConfig::for_payload_limit(payload_blocks * BLOCK as usize);
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
        assert_eq!(
            state.begin_dial(dial(10)),
            Err(SwarmError::PendingDialCapacity)
        );
        state.finish_dial(dial(1)).expect("finish dial");
        state.begin_dial(dial(10)).expect("reused dial slot");
    }

    #[test]
    fn distributes_requests_fairly_and_holds_the_global_payload_bound() {
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
        assert_eq!(snapshot.payload_reserved, 4 * BLOCK as usize);
        assert_eq!(snapshot.payload_high_water, snapshot.payload_reserved);
        assert_eq!(
            snapshot.no_request_reason,
            Some(NoRequestReason::PayloadAllowanceFull)
        );
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
        assert_eq!(state.snapshot(Duration::from_secs(30)).payload_reserved, 0);
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
                .receive_block(first.connection, first.block)
                .expect("late block"),
            ReceiveDisposition::Accept {
                evidence: first.attempt,
                superseded: Some(second.attempt),
                late: true,
            }
        );
        assert_eq!(
            state.snapshot(Duration::from_secs(30)).payload_reserved,
            BLOCK as usize
        );
        state
            .finish_write(first.block, true, Duration::from_secs(30))
            .expect("stored");
        assert_eq!(state.snapshot(Duration::from_secs(30)).payload_reserved, 0);
        assert_eq!(
            state
                .receive_block(second.connection, second.block)
                .expect("duplicate"),
            ReceiveDisposition::Redundant
        );
    }

    #[test]
    fn disconnect_releases_requests_but_not_torrent_owned_writes() {
        let mut state = state(1, vec![plan(0, 2)], 2);
        add_peer(&mut state, connection(1), &[0], false);
        let assigned = state.schedule(Duration::ZERO).expect("schedule");
        state
            .receive_block(connection(1), assigned[0].block)
            .expect("payload");
        let released = state
            .remove_connection(connection(1), ConnectionRemoval::Disconnected)
            .expect("disconnect");
        assert_eq!(released, vec![assigned[1].block]);
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.writing_blocks, 1);
        assert_eq!(snapshot.payload_reserved, BLOCK as usize);
        state
            .finish_write(assigned[0].block, true, Duration::ZERO)
            .expect("write");
        assert_eq!(state.snapshot(Duration::ZERO).payload_reserved, 0);
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
                    .receive_block(request.connection, request.block)
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
    fn full_choked_set_replaces_only_after_grace_and_protects_unique_data() {
        let mut config = SwarmConfig::for_payload_limit(BLOCK as usize);
        config.max_established_connections = 2;
        config.unproductive_grace = Duration::from_secs(30);
        let mut state =
            SwarmState::new(config, 2, vec![plan(0, 1), plan(1, 1)]).expect("swarm state");
        add_peer(&mut state, connection(1), &[0, 1], true);
        add_peer(&mut state, connection(2), &[0], true);
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
        let mut config = SwarmConfig::for_payload_limit(BLOCK as usize);
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
        let mut config = SwarmConfig::for_payload_limit(BLOCK as usize);
        config.max_terminal_attempts_per_block = 2;
        config.max_active_pieces = 1;
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
    fn cancellation_releases_every_reservation_and_pending_dial() {
        let mut state = state(1, vec![plan(0, 2)], 2);
        state.begin_dial(dial(1)).expect("dial");
        add_peer(&mut state, connection(1), &[0], false);
        let assigned = state.schedule(Duration::ZERO).expect("schedule");
        state
            .receive_block(connection(1), assigned[0].block)
            .expect("payload");
        state.cancel_all().expect("cancel");
        let snapshot = state.snapshot(Duration::ZERO);
        assert_eq!(snapshot.pending_dials, 0);
        assert_eq!(snapshot.payload_reserved, 0);
        assert_eq!(snapshot.requested_blocks, 0);
        assert_eq!(snapshot.writing_blocks, 0);
    }
}

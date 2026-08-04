//! Runtime-independent payload upload admission and cancellation state.

use std::collections::VecDeque;

use rstorrent_protocol::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH, PeerMessage};

pub const MAX_QUEUED_UPLOAD_REQUESTS: usize = 32;
pub const MAX_QUEUED_UPLOAD_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadRead {
    pub generation: u64,
    pub request: BlockRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadAction {
    Send(PeerMessage),
    Read(UploadRead),
    Close(UploadCloseReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadCloseReason {
    InvalidRequest,
    RequestLimit,
    ReadFailed,
    ShortRead,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UploadPeerSnapshot {
    pub interested: bool,
    pub choking: bool,
    pub queued_requests: usize,
    pub queued_bytes: usize,
    pub queued_requests_high_water: usize,
    pub queued_bytes_high_water: usize,
    pub read_in_flight: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingRequest {
    generation: u64,
    request: BlockRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InFlightRequest {
    pending: PendingRequest,
    cancelled: bool,
}

#[derive(Debug)]
pub struct UploadPeerState {
    piece_lengths: Vec<u32>,
    available: Vec<bool>,
    interested: bool,
    choking: bool,
    queued: VecDeque<PendingRequest>,
    in_flight: Option<InFlightRequest>,
    pending_bytes: usize,
    next_generation: u64,
    queued_requests_high_water: usize,
    queued_bytes_high_water: usize,
}

impl UploadPeerState {
    pub fn new(piece_lengths: Vec<u32>, available: Vec<bool>) -> Result<Self, &'static str> {
        if piece_lengths.len() != available.len() {
            return Err("piece lengths and availability must have equal lengths");
        }
        if piece_lengths.iter().any(|length| *length == 0) {
            return Err("piece lengths must be nonzero");
        }
        Ok(Self {
            piece_lengths,
            available,
            interested: false,
            choking: true,
            queued: VecDeque::new(),
            in_flight: None,
            pending_bytes: 0,
            next_generation: 1,
            queued_requests_high_water: 0,
            queued_bytes_high_water: 0,
        })
    }

    pub fn bitfield(&self) -> Vec<u8> {
        let mut bitfield = vec![0; self.available.len().div_ceil(8)];
        for (index, available) in self.available.iter().copied().enumerate() {
            if available {
                bitfield[index / 8] |= 1 << (7 - index % 8);
            }
        }
        bitfield
    }

    pub fn snapshot(&self) -> UploadPeerSnapshot {
        UploadPeerSnapshot {
            interested: self.interested,
            choking: self.choking,
            queued_requests: self.pending_count(),
            queued_bytes: self.pending_bytes,
            queued_requests_high_water: self.queued_requests_high_water,
            queued_bytes_high_water: self.queued_bytes_high_water,
            read_in_flight: self.in_flight.is_some(),
        }
    }

    pub fn on_message(&mut self, message: &PeerMessage) -> Vec<UploadAction> {
        match message {
            PeerMessage::Interested => self.on_interested(),
            PeerMessage::NotInterested => self.on_not_interested(),
            PeerMessage::Request(request) => self.on_request(*request),
            PeerMessage::Cancel(request) => self.on_cancel(*request),
            _ => Vec::new(),
        }
    }

    pub fn on_read_complete(
        &mut self,
        read: UploadRead,
        result: Result<Vec<u8>, ()>,
    ) -> Vec<UploadAction> {
        let Some(in_flight) = self.in_flight.take() else {
            return Vec::new();
        };
        if in_flight.pending.generation != read.generation
            || in_flight.pending.request != read.request
        {
            self.in_flight = Some(in_flight);
            return Vec::new();
        }
        self.pending_bytes = self
            .pending_bytes
            .saturating_sub(read.request.length as usize);
        let mut actions = Vec::new();
        match result {
            Err(()) => actions.push(UploadAction::Close(UploadCloseReason::ReadFailed)),
            Ok(block) if block.len() != read.request.length as usize => {
                actions.push(UploadAction::Close(UploadCloseReason::ShortRead));
            }
            Ok(block) if !in_flight.cancelled && self.interested && !self.choking => {
                actions.push(UploadAction::Send(PeerMessage::Piece {
                    index: read.request.index,
                    begin: read.request.begin,
                    block,
                }));
            }
            Ok(_) => {}
        }
        if !actions
            .iter()
            .any(|action| matches!(action, UploadAction::Close(_)))
        {
            self.start_next_read(&mut actions);
        }
        actions
    }

    fn on_interested(&mut self) -> Vec<UploadAction> {
        self.interested = true;
        if self.choking {
            self.choking = false;
            vec![UploadAction::Send(PeerMessage::Unchoke)]
        } else {
            Vec::new()
        }
    }

    fn on_not_interested(&mut self) -> Vec<UploadAction> {
        self.interested = false;
        self.choking = true;
        self.pending_bytes = self
            .in_flight
            .as_ref()
            .map_or(0, |in_flight| in_flight.pending.request.length as usize);
        self.queued.clear();
        if let Some(in_flight) = &mut self.in_flight {
            in_flight.cancelled = true;
        }
        vec![UploadAction::Send(PeerMessage::Choke)]
    }

    fn on_request(&mut self, request: BlockRequest) -> Vec<UploadAction> {
        if self.choking || !self.interested {
            return Vec::new();
        }
        if !self.valid_request(request) {
            return vec![UploadAction::Close(UploadCloseReason::InvalidRequest)];
        }
        let requested_bytes = request.length as usize;
        if self.pending_count() == MAX_QUEUED_UPLOAD_REQUESTS
            || self
                .pending_bytes
                .checked_add(requested_bytes)
                .is_none_or(|bytes| bytes > MAX_QUEUED_UPLOAD_BYTES)
        {
            return vec![UploadAction::Close(UploadCloseReason::RequestLimit)];
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.queued.push_back(PendingRequest {
            generation,
            request,
        });
        self.pending_bytes += requested_bytes;
        self.queued_requests_high_water = self.queued_requests_high_water.max(self.pending_count());
        self.queued_bytes_high_water = self.queued_bytes_high_water.max(self.pending_bytes);
        let mut actions = Vec::new();
        self.start_next_read(&mut actions);
        actions
    }

    fn on_cancel(&mut self, request: BlockRequest) -> Vec<UploadAction> {
        let mut removed_bytes = 0_usize;
        self.queued.retain(|pending| {
            if pending.request == request {
                removed_bytes = removed_bytes.saturating_add(pending.request.length as usize);
                false
            } else {
                true
            }
        });
        self.pending_bytes = self.pending_bytes.saturating_sub(removed_bytes);
        if let Some(in_flight) = &mut self.in_flight
            && in_flight.pending.request == request
        {
            in_flight.cancelled = true;
        }
        Vec::new()
    }

    fn start_next_read(&mut self, actions: &mut Vec<UploadAction>) {
        if self.in_flight.is_some() || self.choking || !self.interested {
            return;
        }
        let Some(pending) = self.queued.pop_front() else {
            return;
        };
        self.in_flight = Some(InFlightRequest {
            pending,
            cancelled: false,
        });
        actions.push(UploadAction::Read(UploadRead {
            generation: pending.generation,
            request: pending.request,
        }));
    }

    fn pending_count(&self) -> usize {
        self.queued.len() + usize::from(self.in_flight.is_some())
    }

    fn valid_request(&self, request: BlockRequest) -> bool {
        if request.length == 0 || request.length > MAX_REQUEST_BLOCK_LENGTH {
            return false;
        }
        let Ok(index) = usize::try_from(request.index) else {
            return false;
        };
        let Some(&piece_length) = self.piece_lengths.get(index) else {
            return false;
        };
        if !self.available.get(index).copied().unwrap_or(false) || request.begin >= piece_length {
            return false;
        }
        request
            .begin
            .checked_add(request.length)
            .is_some_and(|end| end <= piece_length)
    }
}

#[cfg(test)]
mod tests {
    use rstorrent_protocol::peer_wire::{BlockRequest, PeerMessage};

    use super::{
        MAX_QUEUED_UPLOAD_REQUESTS, UploadAction, UploadCloseReason, UploadPeerState, UploadRead,
    };

    fn request(index: u32, begin: u32, length: u32) -> BlockRequest {
        BlockRequest {
            index,
            begin,
            length,
        }
    }

    fn interested(state: &mut UploadPeerState) {
        assert_eq!(
            state.on_message(&PeerMessage::Interested),
            [UploadAction::Send(PeerMessage::Unchoke)]
        );
    }

    #[test]
    fn bitfield_is_exact_and_zeroes_spare_bits() {
        let state = UploadPeerState::new(
            vec![16; 10],
            vec![
                true, false, true, true, false, false, false, true, true, false,
            ],
        )
        .expect("valid state");
        assert_eq!(state.bitfield(), [0b1011_0001, 0b1000_0000]);
    }

    #[test]
    fn ignores_requests_until_interested_and_unchoked() {
        let mut state = UploadPeerState::new(vec![16], vec![true]).expect("valid state");
        assert!(
            state
                .on_message(&PeerMessage::Request(request(0, 0, 16)))
                .is_empty()
        );
        interested(&mut state);
        assert_eq!(
            state.on_message(&PeerMessage::Request(request(0, 0, 16))),
            [UploadAction::Read(UploadRead {
                generation: 1,
                request: request(0, 0, 16),
            })]
        );
    }

    #[test]
    fn validates_every_request_boundary_without_overflow() {
        let mut state =
            UploadPeerState::new(vec![16_384, 7], vec![true, false]).expect("valid state");
        interested(&mut state);
        for invalid in [
            request(0, 0, 0),
            request(0, 0, 16_385),
            request(0, 16_384, 1),
            request(0, 16_383, 2),
            request(1, 0, 1),
            request(2, 0, 1),
            request(0, u32::MAX, 2),
        ] {
            assert_eq!(
                state.on_message(&PeerMessage::Request(invalid)),
                [UploadAction::Close(UploadCloseReason::InvalidRequest)]
            );
        }
    }

    #[test]
    fn duplicate_requests_consume_budget_and_cancel_removes_every_match() {
        let mut state = UploadPeerState::new(vec![16_384], vec![true]).expect("valid state");
        interested(&mut state);
        let block = request(0, 0, 16_384);
        for _ in 0..MAX_QUEUED_UPLOAD_REQUESTS {
            let _ = state.on_message(&PeerMessage::Request(block));
        }
        assert_eq!(state.snapshot().queued_requests, MAX_QUEUED_UPLOAD_REQUESTS);
        assert_eq!(state.snapshot().queued_bytes, 512 * 1024);
        assert_eq!(
            state.on_message(&PeerMessage::Request(block)),
            [UploadAction::Close(UploadCloseReason::RequestLimit)]
        );
        state.on_message(&PeerMessage::Cancel(block));
        assert_eq!(state.snapshot().queued_requests, 1);
        assert_eq!(state.snapshot().queued_bytes, 16_384);
        let actions = state.on_read_complete(
            UploadRead {
                generation: 1,
                request: block,
            },
            Ok(vec![1; 16_384]),
        );
        assert!(actions.is_empty());
        assert_eq!(state.snapshot().queued_requests, 0);
    }

    #[test]
    fn late_or_cancelled_reads_cannot_serialize_payload() {
        let mut state = UploadPeerState::new(vec![4], vec![true]).expect("valid state");
        interested(&mut state);
        let block = request(0, 0, 4);
        let read = match state.on_message(&PeerMessage::Request(block)).as_slice() {
            [UploadAction::Read(read)] => *read,
            actions => panic!("unexpected actions: {actions:?}"),
        };
        assert!(
            state
                .on_read_complete(
                    UploadRead {
                        generation: read.generation + 1,
                        request: block,
                    },
                    Ok(vec![9; 4]),
                )
                .is_empty()
        );
        state.on_message(&PeerMessage::Cancel(block));
        assert!(state.on_read_complete(read, Ok(vec![9; 4])).is_empty());
    }

    #[test]
    fn successful_read_serializes_exact_piece_and_starts_next() {
        let mut state = UploadPeerState::new(vec![8], vec![true]).expect("valid state");
        interested(&mut state);
        let first = request(0, 0, 4);
        let second = request(0, 4, 4);
        state.on_message(&PeerMessage::Request(first));
        assert!(state.on_message(&PeerMessage::Request(second)).is_empty());
        assert_eq!(
            state.on_read_complete(
                UploadRead {
                    generation: 1,
                    request: first,
                },
                Ok(vec![1, 2, 3, 4]),
            ),
            [
                UploadAction::Send(PeerMessage::Piece {
                    index: 0,
                    begin: 0,
                    block: vec![1, 2, 3, 4],
                }),
                UploadAction::Read(UploadRead {
                    generation: 2,
                    request: second,
                }),
            ]
        );
    }

    #[test]
    fn read_failures_and_short_reads_close_without_payload() {
        for (result, reason) in [
            (Err(()), UploadCloseReason::ReadFailed),
            (Ok(vec![1; 3]), UploadCloseReason::ShortRead),
        ] {
            let mut state = UploadPeerState::new(vec![4], vec![true]).expect("valid state");
            interested(&mut state);
            let block = request(0, 0, 4);
            state.on_message(&PeerMessage::Request(block));
            assert_eq!(
                state.on_read_complete(
                    UploadRead {
                        generation: 1,
                        request: block,
                    },
                    result,
                ),
                [UploadAction::Close(reason)]
            );
        }
    }

    #[test]
    fn not_interested_chokes_clears_queue_and_suppresses_read() {
        let mut state = UploadPeerState::new(vec![8], vec![true]).expect("valid state");
        interested(&mut state);
        state.on_message(&PeerMessage::Request(request(0, 0, 4)));
        state.on_message(&PeerMessage::Request(request(0, 4, 4)));
        assert_eq!(
            state.on_message(&PeerMessage::NotInterested),
            [UploadAction::Send(PeerMessage::Choke)]
        );
        assert_eq!(state.snapshot().queued_requests, 1);
        assert!(
            state
                .on_read_complete(
                    UploadRead {
                        generation: 1,
                        request: request(0, 0, 4),
                    },
                    Ok(vec![1; 4]),
                )
                .is_empty()
        );
    }
}

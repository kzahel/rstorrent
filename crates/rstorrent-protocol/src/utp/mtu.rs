//! Deterministic packetization-layer path-MTU discovery state.

use std::error::Error;
use std::fmt;

use super::SequenceNumber;

pub const MIN_UTP_DATAGRAM_BYTES: usize = 150;
pub const IPV4_UDP_PAYLOAD_FLOOR: usize = 548;
pub const IPV4_UDP_PAYLOAD_CEILING: usize = 1_472;
pub const MTU_SEARCH_THRESHOLD_BYTES: usize = 16;
const ORDINARY_PACKETS_BEFORE_PROBE: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MtuProbe {
    pub sequence_number: SequenceNumber,
    pub datagram_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MtuProbeFailure {
    ThreeLaterAcknowledgements,
    SolePacketTimeout,
    CongestionOrUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MtuProbeOutcome {
    NotProbe,
    RaisedFloor {
        previous_floor: usize,
        floor: usize,
        search_complete: bool,
    },
    LoweredCeiling {
        previous_ceiling: usize,
        ceiling: usize,
        retransmit_without_fragmentation_protection: MtuProbe,
        isolated_from_congestion: bool,
        search_complete: bool,
    },
    RetryAcknowledged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathMtuSnapshot {
    pub floor_datagram_bytes: usize,
    pub ceiling_datagram_bytes: usize,
    pub candidate_datagram_bytes: usize,
    pub search_complete: bool,
    pub ordinary_packets_since_probe: u8,
    pub active_probe: Option<MtuProbe>,
    pub fragmentable_retry: Option<MtuProbe>,
    pub probes_started: u64,
    pub probes_acknowledged: u64,
    pub probes_failed: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MtuError {
    InvalidBounds {
        floor: usize,
        ceiling: usize,
    },
    ProbeNotReady,
    ProbeAlreadyActive(MtuProbe),
    DatagramSizeMismatch {
        expected: usize,
        actual: usize,
    },
    MinimumMtuFailure {
        datagram_bytes: usize,
        floor: usize,
    },
    SequencedPacketTooLarge {
        sequence_number: SequenceNumber,
        datagram_bytes: usize,
        floor: usize,
    },
}

impl fmt::Display for MtuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBounds { floor, ceiling } => write!(
                formatter,
                "uTP path-MTU bounds require {MIN_UTP_DATAGRAM_BYTES} <= floor <= ceiling <= 65535, got {floor}..={ceiling}"
            ),
            Self::ProbeNotReady => formatter.write_str("uTP path-MTU probe is not ready"),
            Self::ProbeAlreadyActive(probe) => write!(
                formatter,
                "uTP path-MTU probe {} at {} bytes is still active",
                probe.sequence_number.get(),
                probe.datagram_bytes
            ),
            Self::DatagramSizeMismatch { expected, actual } => write!(
                formatter,
                "uTP path-MTU probe datagram {actual} does not match candidate {expected}"
            ),
            Self::MinimumMtuFailure {
                datagram_bytes,
                floor,
            } => write!(
                formatter,
                "uTP proven minimum datagram {datagram_bytes} failed below path-MTU floor {floor}"
            ),
            Self::SequencedPacketTooLarge {
                sequence_number,
                datagram_bytes,
                floor,
            } => write!(
                formatter,
                "uTP sequenced packet {} at {datagram_bytes} bytes cannot be repacketized around floor {floor}",
                sequence_number.get()
            ),
        }
    }
}

impl Error for MtuError {}

#[derive(Clone, Debug)]
pub struct PathMtuState {
    floor_datagram_bytes: usize,
    ceiling_datagram_bytes: usize,
    ordinary_packets_since_probe: u8,
    active_probe: Option<MtuProbe>,
    fragmentable_retry: Option<MtuProbe>,
    probes_started: u64,
    probes_acknowledged: u64,
    probes_failed: u64,
}

impl PathMtuState {
    pub fn new(
        floor_datagram_bytes: usize,
        ceiling_datagram_bytes: usize,
    ) -> Result<Self, MtuError> {
        if floor_datagram_bytes < MIN_UTP_DATAGRAM_BYTES
            || floor_datagram_bytes > ceiling_datagram_bytes
            || ceiling_datagram_bytes > 65_535
        {
            return Err(MtuError::InvalidBounds {
                floor: floor_datagram_bytes,
                ceiling: ceiling_datagram_bytes,
            });
        }
        Ok(Self {
            floor_datagram_bytes,
            ceiling_datagram_bytes,
            ordinary_packets_since_probe: 0,
            active_probe: None,
            fragmentable_retry: None,
            probes_started: 0,
            probes_acknowledged: 0,
            probes_failed: 0,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> PathMtuSnapshot {
        PathMtuSnapshot {
            floor_datagram_bytes: self.floor_datagram_bytes,
            ceiling_datagram_bytes: self.ceiling_datagram_bytes,
            candidate_datagram_bytes: self.candidate_datagram_bytes(),
            search_complete: self.search_complete(),
            ordinary_packets_since_probe: self.ordinary_packets_since_probe,
            active_probe: self.active_probe,
            fragmentable_retry: self.fragmentable_retry,
            probes_started: self.probes_started,
            probes_acknowledged: self.probes_acknowledged,
            probes_failed: self.probes_failed,
        }
    }

    #[must_use]
    pub fn ordinary_datagram_bytes(&self) -> usize {
        self.floor_datagram_bytes
    }

    #[must_use]
    pub fn candidate_datagram_bytes(&self) -> usize {
        self.floor_datagram_bytes + (self.ceiling_datagram_bytes - self.floor_datagram_bytes) / 2
    }

    #[must_use]
    pub fn search_complete(&self) -> bool {
        self.ceiling_datagram_bytes - self.floor_datagram_bytes <= MTU_SEARCH_THRESHOLD_BYTES
    }

    pub fn record_ordinary_sent(&mut self, datagram_bytes: usize) {
        if datagram_bytes >= self.floor_datagram_bytes {
            self.ordinary_packets_since_probe = self
                .ordinary_packets_since_probe
                .saturating_add(1)
                .min(ORDINARY_PACKETS_BEFORE_PROBE);
        }
    }

    #[must_use]
    pub fn probe_ready(&self, congestion_window_bytes: usize) -> bool {
        !self.search_complete()
            && self.active_probe.is_none()
            && self.fragmentable_retry.is_none()
            && self.ordinary_packets_since_probe >= ORDINARY_PACKETS_BEFORE_PROBE
            && congestion_window_bytes > self.floor_datagram_bytes.saturating_mul(3)
    }

    pub fn begin_probe(
        &mut self,
        sequence_number: SequenceNumber,
        datagram_bytes: usize,
        congestion_window_bytes: usize,
    ) -> Result<MtuProbe, MtuError> {
        if let Some(active) = self.active_probe {
            return Err(MtuError::ProbeAlreadyActive(active));
        }
        if !self.probe_ready(congestion_window_bytes) {
            return Err(MtuError::ProbeNotReady);
        }
        let expected = self.candidate_datagram_bytes();
        if datagram_bytes != expected {
            return Err(MtuError::DatagramSizeMismatch {
                expected,
                actual: datagram_bytes,
            });
        }
        let probe = MtuProbe {
            sequence_number,
            datagram_bytes,
        };
        self.active_probe = Some(probe);
        self.ordinary_packets_since_probe = 0;
        self.probes_started = self.probes_started.saturating_add(1);
        Ok(probe)
    }

    pub fn on_acknowledged(&mut self, sequence_number: SequenceNumber) -> MtuProbeOutcome {
        if self
            .fragmentable_retry
            .is_some_and(|retry| retry.sequence_number == sequence_number)
        {
            self.fragmentable_retry = None;
            return MtuProbeOutcome::RetryAcknowledged;
        }
        let Some(probe) = self.active_probe else {
            return MtuProbeOutcome::NotProbe;
        };
        if probe.sequence_number != sequence_number {
            return MtuProbeOutcome::NotProbe;
        }
        let previous_floor = self.floor_datagram_bytes;
        self.floor_datagram_bytes = self.floor_datagram_bytes.max(probe.datagram_bytes);
        self.active_probe = None;
        self.probes_acknowledged = self.probes_acknowledged.saturating_add(1);
        MtuProbeOutcome::RaisedFloor {
            previous_floor,
            floor: self.floor_datagram_bytes,
            search_complete: self.search_complete(),
        }
    }

    pub fn on_probe_loss(
        &mut self,
        sequence_number: SequenceNumber,
        failure: MtuProbeFailure,
    ) -> MtuProbeOutcome {
        let Some(probe) = self.active_probe else {
            return MtuProbeOutcome::NotProbe;
        };
        if probe.sequence_number != sequence_number {
            return MtuProbeOutcome::NotProbe;
        }
        let previous_ceiling = self.ceiling_datagram_bytes;
        let isolated_from_congestion = matches!(
            failure,
            MtuProbeFailure::ThreeLaterAcknowledgements | MtuProbeFailure::SolePacketTimeout
        );
        if isolated_from_congestion {
            self.ceiling_datagram_bytes = self
                .ceiling_datagram_bytes
                .min(probe.datagram_bytes.saturating_sub(1))
                .max(self.floor_datagram_bytes);
            self.probes_failed = self.probes_failed.saturating_add(1);
        }
        self.active_probe = None;
        self.fragmentable_retry = Some(probe);
        self.ordinary_packets_since_probe = 0;
        MtuProbeOutcome::LoweredCeiling {
            previous_ceiling,
            ceiling: self.ceiling_datagram_bytes,
            retransmit_without_fragmentation_protection: probe,
            isolated_from_congestion,
            search_complete: self.search_complete(),
        }
    }

    pub fn on_message_too_large(
        &mut self,
        sequence_number: SequenceNumber,
        datagram_bytes: usize,
    ) -> Result<MtuProbeOutcome, MtuError> {
        if self.active_probe.is_some_and(|probe| {
            probe.sequence_number == sequence_number && probe.datagram_bytes == datagram_bytes
        }) {
            return Ok(
                self.on_probe_loss(sequence_number, MtuProbeFailure::ThreeLaterAcknowledgements)
            );
        }
        if datagram_bytes <= self.floor_datagram_bytes {
            return Err(MtuError::MinimumMtuFailure {
                datagram_bytes,
                floor: self.floor_datagram_bytes,
            });
        }
        Err(MtuError::SequencedPacketTooLarge {
            sequence_number,
            datagram_bytes,
            floor: self.floor_datagram_bytes,
        })
    }

    #[must_use]
    pub fn retransmit_without_fragmentation_protection(
        &self,
        sequence_number: SequenceNumber,
    ) -> bool {
        self.fragmentable_retry
            .is_some_and(|probe| probe.sequence_number == sequence_number)
    }

    pub fn reset(&mut self) {
        self.active_probe = None;
        self.fragmentable_retry = None;
        self.ordinary_packets_since_probe = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence(value: u16) -> SequenceNumber {
        SequenceNumber::new(value)
    }

    fn ready(state: &mut PathMtuState) {
        for _ in 0..ORDINARY_PACKETS_BEFORE_PROBE {
            state.record_ordinary_sent(state.ordinary_datagram_bytes());
        }
    }

    #[test]
    fn bounds_are_explicit_and_atomic() {
        assert!(matches!(
            PathMtuState::new(149, 1_472),
            Err(MtuError::InvalidBounds { .. })
        ));
        assert!(matches!(
            PathMtuState::new(1_472, 548),
            Err(MtuError::InvalidBounds { .. })
        ));
        let state = PathMtuState::new(548, 1_472).expect("valid bounds");
        assert_eq!(state.snapshot().candidate_datagram_bytes, 1_010);
        assert!(!state.snapshot().search_complete);
    }

    #[test]
    fn probe_requires_three_floor_packets_and_three_floor_windows() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        assert!(!state.probe_ready(10_000));
        ready(&mut state);
        assert!(!state.probe_ready(548 * 3));
        assert!(state.probe_ready(548 * 3 + 1));
        let before = state.snapshot();
        assert!(matches!(
            state.begin_probe(sequence(1), 1_011, 10_000),
            Err(MtuError::DatagramSizeMismatch { .. })
        ));
        assert_eq!(state.snapshot(), before);

        let probe = state
            .begin_probe(sequence(1), 1_010, 10_000)
            .expect("begin probe");
        assert_eq!(probe.datagram_bytes, 1_010);
        let outcome = state.on_acknowledged(sequence(1));
        assert_eq!(
            outcome,
            MtuProbeOutcome::RaisedFloor {
                previous_floor: 548,
                floor: 1_010,
                search_complete: false,
            }
        );
        assert_eq!(state.snapshot().probes_acknowledged, 1);
    }

    #[test]
    fn isolated_probe_loss_lowers_ceiling_and_preserves_sequence_retry() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        ready(&mut state);
        state
            .begin_probe(sequence(u16::MAX), 1_010, 10_000)
            .expect("probe");
        let outcome = state.on_probe_loss(
            sequence(u16::MAX),
            MtuProbeFailure::ThreeLaterAcknowledgements,
        );
        assert!(matches!(
            outcome,
            MtuProbeOutcome::LoweredCeiling {
                previous_ceiling: 1_472,
                ceiling: 1_009,
                isolated_from_congestion: true,
                ..
            }
        ));
        assert!(state.retransmit_without_fragmentation_protection(sequence(u16::MAX)));
        assert!(!state.probe_ready(10_000));
        assert_eq!(
            state.on_acknowledged(sequence(u16::MAX)),
            MtuProbeOutcome::RetryAcknowledged
        );
        assert!(!state.retransmit_without_fragmentation_protection(sequence(u16::MAX)));
        assert_eq!(state.snapshot().floor_datagram_bytes, 548);
    }

    #[test]
    fn unknown_probe_loss_retries_without_claiming_a_smaller_path() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        ready(&mut state);
        state
            .begin_probe(sequence(2), 1_010, 10_000)
            .expect("probe");
        let outcome = state.on_probe_loss(sequence(2), MtuProbeFailure::CongestionOrUnknown);
        assert!(matches!(
            outcome,
            MtuProbeOutcome::LoweredCeiling {
                ceiling: 1_472,
                isolated_from_congestion: false,
                ..
            }
        ));
        assert_eq!(state.snapshot().probes_failed, 0);
    }

    #[test]
    fn binary_search_converges_under_a_df_black_hole() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        let path_limit = 1_280;
        let mut outcomes = 0;
        while !state.search_complete() {
            ready(&mut state);
            let probe_size = state.candidate_datagram_bytes();
            let sequence_number = sequence(outcomes);
            state
                .begin_probe(sequence_number, probe_size, 10_000)
                .expect("probe");
            if probe_size <= path_limit {
                assert!(matches!(
                    state.on_acknowledged(sequence_number),
                    MtuProbeOutcome::RaisedFloor { .. }
                ));
            } else {
                assert!(matches!(
                    state.on_probe_loss(sequence_number, MtuProbeFailure::SolePacketTimeout),
                    MtuProbeOutcome::LoweredCeiling {
                        isolated_from_congestion: true,
                        ..
                    }
                ));
                assert_eq!(
                    state.on_acknowledged(sequence_number),
                    MtuProbeOutcome::RetryAcknowledged
                );
            }
            outcomes += 1;
            assert!(outcomes <= 10);
        }
        let snapshot = state.snapshot();
        assert!(snapshot.floor_datagram_bytes <= path_limit);
        assert!(path_limit - snapshot.floor_datagram_bytes <= MTU_SEARCH_THRESHOLD_BYTES);
        assert!(snapshot.ceiling_datagram_bytes - snapshot.floor_datagram_bytes <= 16);
    }

    #[test]
    fn local_too_large_distinguishes_probe_minimum_and_sequenced_packet() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        ready(&mut state);
        state
            .begin_probe(sequence(1), 1_010, 10_000)
            .expect("probe");
        assert!(matches!(
            state
                .on_message_too_large(sequence(1), 1_010)
                .expect("probe result"),
            MtuProbeOutcome::LoweredCeiling {
                isolated_from_congestion: true,
                ..
            }
        ));
        assert!(matches!(
            state.on_message_too_large(sequence(2), 548),
            Err(MtuError::MinimumMtuFailure { .. })
        ));
        assert!(matches!(
            state.on_message_too_large(sequence(1), 1_010),
            Err(MtuError::SequencedPacketTooLarge { .. })
        ));
    }
}

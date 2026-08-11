//! Deterministic packetization-layer path-MTU discovery state.

use std::error::Error;
use std::fmt;

use super::SequenceNumber;

pub const MIN_UTP_DATAGRAM_BYTES: usize = 150;
pub const IPV4_UDP_PAYLOAD_FLOOR: usize = 548;
pub const IPV4_UDP_PAYLOAD_CEILING: usize = 1_472;
pub const MTU_SEARCH_THRESHOLD_BYTES: usize = 16;
pub const PATH_MTU_REVALIDATION_INTERVAL_MICROS: u64 = 15 * 60 * 1_000_000;
const ORDINARY_PACKETS_BEFORE_PROBE: u8 = 3;
const UNKNOWN_RTT_PROBE_GUARD_MICROS: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathMtuPhase {
    Base,
    Search,
    SearchComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MtuProbeKind {
    Search,
    Revalidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MtuProbe {
    pub sequence_number: SequenceNumber,
    pub datagram_bytes: usize,
    pub kind: MtuProbeKind,
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
    RevalidationAcknowledged {
        floor: usize,
        next_revalidation_micros: u64,
    },
    LoweredCeiling {
        previous_floor: usize,
        floor: usize,
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
    pub phase: PathMtuPhase,
    pub base_datagram_bytes: usize,
    pub maximum_datagram_bytes: usize,
    pub floor_datagram_bytes: usize,
    pub ceiling_datagram_bytes: usize,
    pub candidate_datagram_bytes: usize,
    pub search_complete: bool,
    pub ordinary_packets_since_probe: u8,
    pub active_probe: Option<MtuProbe>,
    pub fragmentable_retry: Option<MtuProbe>,
    pub next_probe_not_before_micros: u64,
    pub revalidation_deadline_micros: Option<u64>,
    pub probes_started: u64,
    pub probes_acknowledged: u64,
    pub probes_failed: u64,
    pub revalidations_started: u64,
    pub revalidations_acknowledged: u64,
    pub revalidations_failed: u64,
    pub downward_recoveries: u64,
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
    phase: PathMtuPhase,
    base_datagram_bytes: usize,
    maximum_datagram_bytes: usize,
    floor_datagram_bytes: usize,
    ceiling_datagram_bytes: usize,
    ordinary_packets_since_probe: u8,
    active_probe: Option<MtuProbe>,
    fragmentable_retry: Option<MtuProbe>,
    next_probe_not_before_micros: u64,
    revalidation_deadline_micros: Option<u64>,
    probes_started: u64,
    probes_acknowledged: u64,
    probes_failed: u64,
    revalidations_started: u64,
    revalidations_acknowledged: u64,
    revalidations_failed: u64,
    downward_recoveries: u64,
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
        let phase = if ceiling_datagram_bytes - floor_datagram_bytes <= MTU_SEARCH_THRESHOLD_BYTES {
            PathMtuPhase::SearchComplete
        } else {
            PathMtuPhase::Base
        };
        Ok(Self {
            phase,
            base_datagram_bytes: floor_datagram_bytes,
            maximum_datagram_bytes: ceiling_datagram_bytes,
            floor_datagram_bytes,
            ceiling_datagram_bytes,
            ordinary_packets_since_probe: 0,
            active_probe: None,
            fragmentable_retry: None,
            next_probe_not_before_micros: 0,
            revalidation_deadline_micros: None,
            probes_started: 0,
            probes_acknowledged: 0,
            probes_failed: 0,
            revalidations_started: 0,
            revalidations_acknowledged: 0,
            revalidations_failed: 0,
            downward_recoveries: 0,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> PathMtuSnapshot {
        PathMtuSnapshot {
            phase: self.phase,
            base_datagram_bytes: self.base_datagram_bytes,
            maximum_datagram_bytes: self.maximum_datagram_bytes,
            floor_datagram_bytes: self.floor_datagram_bytes,
            ceiling_datagram_bytes: self.ceiling_datagram_bytes,
            candidate_datagram_bytes: self.candidate_datagram_bytes(),
            search_complete: self.search_complete(),
            ordinary_packets_since_probe: self.ordinary_packets_since_probe,
            active_probe: self.active_probe,
            fragmentable_retry: self.fragmentable_retry,
            next_probe_not_before_micros: self.next_probe_not_before_micros,
            revalidation_deadline_micros: self.revalidation_deadline_micros,
            probes_started: self.probes_started,
            probes_acknowledged: self.probes_acknowledged,
            probes_failed: self.probes_failed,
            revalidations_started: self.revalidations_started,
            revalidations_acknowledged: self.revalidations_acknowledged,
            revalidations_failed: self.revalidations_failed,
            downward_recoveries: self.downward_recoveries,
        }
    }

    #[must_use]
    pub fn ordinary_datagram_bytes(&self) -> usize {
        self.floor_datagram_bytes
    }

    #[must_use]
    pub fn candidate_datagram_bytes(&self) -> usize {
        if self.phase == PathMtuPhase::SearchComplete {
            self.floor_datagram_bytes
        } else {
            self.floor_datagram_bytes
                + (self.ceiling_datagram_bytes - self.floor_datagram_bytes) / 2
        }
    }

    #[must_use]
    pub fn search_complete(&self) -> bool {
        self.phase == PathMtuPhase::SearchComplete
    }

    pub fn record_ordinary_sent(&mut self, datagram_bytes: usize, now_micros: u64) {
        if datagram_bytes >= self.floor_datagram_bytes {
            self.ordinary_packets_since_probe = self
                .ordinary_packets_since_probe
                .saturating_add(1)
                .min(ORDINARY_PACKETS_BEFORE_PROBE);
        }
        if self.phase == PathMtuPhase::SearchComplete
            && self.maximum_datagram_bytes > self.base_datagram_bytes
            && self.active_probe.is_none()
            && self.fragmentable_retry.is_none()
            && self.revalidation_deadline_micros.is_none()
        {
            self.schedule_revalidation(now_micros);
        }
    }

    #[must_use]
    pub fn probe_ready(&self, now_micros: u64, congestion_window_bytes: usize) -> bool {
        let phase_ready = match self.phase {
            PathMtuPhase::Base | PathMtuPhase::Search => true,
            PathMtuPhase::SearchComplete => {
                self.maximum_datagram_bytes > self.base_datagram_bytes
                    && self
                        .revalidation_deadline_micros
                        .is_some_and(|deadline| now_micros >= deadline)
            }
        };
        phase_ready
            && self.active_probe.is_none()
            && self.fragmentable_retry.is_none()
            && now_micros >= self.next_probe_not_before_micros
            && self.ordinary_packets_since_probe >= ORDINARY_PACKETS_BEFORE_PROBE
            && congestion_window_bytes > self.floor_datagram_bytes.saturating_mul(3)
    }

    pub fn begin_probe(
        &mut self,
        sequence_number: SequenceNumber,
        datagram_bytes: usize,
        now_micros: u64,
        congestion_window_bytes: usize,
    ) -> Result<MtuProbe, MtuError> {
        if let Some(active) = self.active_probe {
            return Err(MtuError::ProbeAlreadyActive(active));
        }
        if !self.probe_ready(now_micros, congestion_window_bytes) {
            return Err(MtuError::ProbeNotReady);
        }
        let expected = self.candidate_datagram_bytes();
        if datagram_bytes != expected {
            return Err(MtuError::DatagramSizeMismatch {
                expected,
                actual: datagram_bytes,
            });
        }
        let kind = if self.phase == PathMtuPhase::SearchComplete {
            MtuProbeKind::Revalidation
        } else {
            MtuProbeKind::Search
        };
        let probe = MtuProbe {
            sequence_number,
            datagram_bytes,
            kind,
        };
        self.active_probe = Some(probe);
        if kind == MtuProbeKind::Search {
            self.phase = PathMtuPhase::Search;
        } else {
            self.revalidation_deadline_micros = None;
            self.revalidations_started = self.revalidations_started.saturating_add(1);
        }
        self.ordinary_packets_since_probe = 0;
        self.probes_started = self.probes_started.saturating_add(1);
        Ok(probe)
    }

    pub fn on_acknowledged(
        &mut self,
        sequence_number: SequenceNumber,
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
    ) -> MtuProbeOutcome {
        if self
            .fragmentable_retry
            .is_some_and(|retry| retry.sequence_number == sequence_number)
        {
            self.fragmentable_retry = None;
            self.guard_next_probe(now_micros, smoothed_rtt_micros);
            if self.phase == PathMtuPhase::SearchComplete {
                self.schedule_revalidation(now_micros);
            }
            return MtuProbeOutcome::RetryAcknowledged;
        }
        let Some(probe) = self.active_probe else {
            return MtuProbeOutcome::NotProbe;
        };
        if probe.sequence_number != sequence_number {
            return MtuProbeOutcome::NotProbe;
        }
        self.active_probe = None;
        self.probes_acknowledged = self.probes_acknowledged.saturating_add(1);
        self.guard_next_probe(now_micros, smoothed_rtt_micros);
        if probe.kind == MtuProbeKind::Revalidation {
            self.revalidations_acknowledged = self.revalidations_acknowledged.saturating_add(1);
            self.schedule_revalidation(now_micros);
            return MtuProbeOutcome::RevalidationAcknowledged {
                floor: self.floor_datagram_bytes,
                next_revalidation_micros: self
                    .revalidation_deadline_micros
                    .expect("revalidation acknowledgement schedules its successor"),
            };
        }
        let previous_floor = self.floor_datagram_bytes;
        self.floor_datagram_bytes = self.floor_datagram_bytes.max(probe.datagram_bytes);
        self.update_phase_after_search(now_micros);
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
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
    ) -> MtuProbeOutcome {
        let Some(probe) = self.active_probe else {
            return MtuProbeOutcome::NotProbe;
        };
        if probe.sequence_number != sequence_number {
            return MtuProbeOutcome::NotProbe;
        }
        let previous_floor = self.floor_datagram_bytes;
        let previous_ceiling = self.ceiling_datagram_bytes;
        let isolated_from_congestion = matches!(
            failure,
            MtuProbeFailure::ThreeLaterAcknowledgements | MtuProbeFailure::SolePacketTimeout
        );
        if isolated_from_congestion {
            if probe.kind == MtuProbeKind::Revalidation {
                self.ceiling_datagram_bytes = probe
                    .datagram_bytes
                    .saturating_sub(1)
                    .max(self.base_datagram_bytes);
                self.floor_datagram_bytes = self.base_datagram_bytes;
                if self.floor_datagram_bytes != previous_floor {
                    self.downward_recoveries = self.downward_recoveries.saturating_add(1);
                }
                self.revalidations_failed = self.revalidations_failed.saturating_add(1);
            } else {
                self.ceiling_datagram_bytes = self
                    .ceiling_datagram_bytes
                    .min(probe.datagram_bytes.saturating_sub(1))
                    .max(self.floor_datagram_bytes);
            }
            self.probes_failed = self.probes_failed.saturating_add(1);
        }
        self.active_probe = None;
        self.fragmentable_retry = Some(probe);
        self.ordinary_packets_since_probe = 0;
        self.guard_next_probe(now_micros, smoothed_rtt_micros);
        if probe.kind == MtuProbeKind::Revalidation && !isolated_from_congestion {
            self.schedule_revalidation(now_micros);
        } else if isolated_from_congestion {
            self.update_phase_after_search(now_micros);
        }
        MtuProbeOutcome::LoweredCeiling {
            previous_floor,
            floor: self.floor_datagram_bytes,
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
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
    ) -> Result<MtuProbeOutcome, MtuError> {
        if self.active_probe.is_some_and(|probe| {
            probe.sequence_number == sequence_number && probe.datagram_bytes == datagram_bytes
        }) {
            return Ok(self.on_probe_loss(
                sequence_number,
                MtuProbeFailure::ThreeLaterAcknowledgements,
                now_micros,
                smoothed_rtt_micros,
            ));
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

    fn update_phase_after_search(&mut self, now_micros: u64) {
        if self.ceiling_datagram_bytes - self.floor_datagram_bytes <= MTU_SEARCH_THRESHOLD_BYTES {
            self.phase = PathMtuPhase::SearchComplete;
            self.schedule_revalidation(now_micros);
        } else {
            self.phase = PathMtuPhase::Search;
            self.revalidation_deadline_micros = None;
        }
    }

    fn guard_next_probe(&mut self, now_micros: u64, smoothed_rtt_micros: Option<u64>) {
        self.next_probe_not_before_micros = now_micros
            .saturating_add(smoothed_rtt_micros.unwrap_or(UNKNOWN_RTT_PROBE_GUARD_MICROS));
    }

    fn schedule_revalidation(&mut self, now_micros: u64) {
        self.revalidation_deadline_micros =
            Some(now_micros.saturating_add(PATH_MTU_REVALIDATION_INTERVAL_MICROS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence(value: u16) -> SequenceNumber {
        SequenceNumber::new(value)
    }

    fn ready(state: &mut PathMtuState, now_micros: u64) {
        for _ in 0..ORDINARY_PACKETS_BEFORE_PROBE {
            state.record_ordinary_sent(state.ordinary_datagram_bytes(), now_micros);
        }
    }

    fn converge_clean_path(state: &mut PathMtuState, mut now_micros: u64) -> u64 {
        let mut sequence_number = 0;
        while !state.search_complete() {
            ready(state, now_micros);
            let candidate = state.candidate_datagram_bytes();
            state
                .begin_probe(sequence(sequence_number), candidate, now_micros, 10_000)
                .expect("begin clean-path probe");
            now_micros = now_micros.saturating_add(2_000_000);
            assert!(matches!(
                state.on_acknowledged(sequence(sequence_number), now_micros, Some(100_000)),
                MtuProbeOutcome::RaisedFloor { .. }
            ));
            now_micros = now_micros.saturating_add(2_000_000);
            sequence_number += 1;
            assert!(sequence_number <= 10);
        }
        now_micros
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
        assert_eq!(state.snapshot().phase, PathMtuPhase::Base);
        assert!(!state.snapshot().search_complete);
    }

    #[test]
    fn probe_requires_three_floor_packets_and_three_floor_windows() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        assert!(!state.probe_ready(0, 10_000));
        ready(&mut state, 0);
        assert!(!state.probe_ready(0, 548 * 3));
        assert!(state.probe_ready(0, 548 * 3 + 1));
        let before = state.snapshot();
        assert!(matches!(
            state.begin_probe(sequence(1), 1_011, 0, 10_000),
            Err(MtuError::DatagramSizeMismatch { .. })
        ));
        assert_eq!(state.snapshot(), before);

        let probe = state
            .begin_probe(sequence(1), 1_010, 0, 10_000)
            .expect("begin probe");
        assert_eq!(probe.datagram_bytes, 1_010);
        let outcome = state.on_acknowledged(sequence(1), 10, Some(100));
        assert_eq!(
            outcome,
            MtuProbeOutcome::RaisedFloor {
                previous_floor: 548,
                floor: 1_010,
                search_complete: false,
            }
        );
        assert_eq!(state.snapshot().probes_acknowledged, 1);
        ready(&mut state, 10);
        assert!(!state.probe_ready(109, 10_000));
        assert!(state.probe_ready(110, 10_000));
    }

    #[test]
    fn isolated_probe_loss_lowers_ceiling_and_preserves_sequence_retry() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        ready(&mut state, 0);
        state
            .begin_probe(sequence(u16::MAX), 1_010, 0, 10_000)
            .expect("probe");
        let outcome = state.on_probe_loss(
            sequence(u16::MAX),
            MtuProbeFailure::ThreeLaterAcknowledgements,
            10,
            Some(100),
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
        assert!(!state.probe_ready(10, 10_000));
        assert_eq!(
            state.on_acknowledged(sequence(u16::MAX), 20, Some(100)),
            MtuProbeOutcome::RetryAcknowledged
        );
        assert!(!state.retransmit_without_fragmentation_protection(sequence(u16::MAX)));
        assert_eq!(state.snapshot().floor_datagram_bytes, 548);
    }

    #[test]
    fn unknown_probe_loss_retries_without_claiming_a_smaller_path() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        ready(&mut state, 0);
        state
            .begin_probe(sequence(2), 1_010, 0, 10_000)
            .expect("probe");
        let outcome = state.on_probe_loss(
            sequence(2),
            MtuProbeFailure::CongestionOrUnknown,
            10,
            Some(100),
        );
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
        let mut now_micros = 0;
        while !state.search_complete() {
            ready(&mut state, now_micros);
            let probe_size = state.candidate_datagram_bytes();
            let sequence_number = sequence(outcomes);
            state
                .begin_probe(sequence_number, probe_size, now_micros, 10_000)
                .expect("probe");
            now_micros += 2_000_000;
            if probe_size <= path_limit {
                assert!(matches!(
                    state.on_acknowledged(sequence_number, now_micros, Some(100_000)),
                    MtuProbeOutcome::RaisedFloor { .. }
                ));
            } else {
                assert!(matches!(
                    state.on_probe_loss(
                        sequence_number,
                        MtuProbeFailure::SolePacketTimeout,
                        now_micros,
                        Some(100_000),
                    ),
                    MtuProbeOutcome::LoweredCeiling {
                        isolated_from_congestion: true,
                        ..
                    }
                ));
                assert_eq!(
                    state.on_acknowledged(sequence_number, now_micros, Some(100_000)),
                    MtuProbeOutcome::RetryAcknowledged
                );
            }
            now_micros += 2_000_000;
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
        ready(&mut state, 0);
        state
            .begin_probe(sequence(1), 1_010, 0, 10_000)
            .expect("probe");
        assert!(matches!(
            state
                .on_message_too_large(sequence(1), 1_010, 10, Some(100))
                .expect("probe result"),
            MtuProbeOutcome::LoweredCeiling {
                isolated_from_congestion: true,
                ..
            }
        ));
        assert!(matches!(
            state.on_message_too_large(sequence(2), 548, 20, Some(100)),
            Err(MtuError::MinimumMtuFailure { .. })
        ));
        assert!(matches!(
            state.on_message_too_large(sequence(1), 1_010, 20, Some(100)),
            Err(MtuError::SequencedPacketTooLarge { .. })
        ));
    }

    #[test]
    fn completed_search_revalidates_only_after_fifteen_minutes() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        let completed_at = converge_clean_path(&mut state, 0);
        let completed = state.snapshot();
        let deadline = completed
            .revalidation_deadline_micros
            .expect("completed search deadline");
        assert_eq!(completed.phase, PathMtuPhase::SearchComplete);
        assert!(deadline <= completed_at + PATH_MTU_REVALIDATION_INTERVAL_MICROS);
        ready(&mut state, completed_at);
        assert!(!state.probe_ready(deadline - 1, 10_000));
        assert!(state.probe_ready(deadline, 10_000));

        let floor = state.ordinary_datagram_bytes();
        let probe = state
            .begin_probe(sequence(20), floor, deadline, 10_000)
            .expect("begin revalidation");
        assert_eq!(probe.kind, MtuProbeKind::Revalidation);
        assert_eq!(probe.datagram_bytes, floor);
        assert_eq!(
            state.on_acknowledged(sequence(20), deadline + 100, Some(100)),
            MtuProbeOutcome::RevalidationAcknowledged {
                floor,
                next_revalidation_micros: deadline + 100 + PATH_MTU_REVALIDATION_INTERVAL_MICROS,
            }
        );
        let revalidated = state.snapshot();
        assert_eq!(revalidated.floor_datagram_bytes, floor);
        assert_eq!(revalidated.revalidations_started, 1);
        assert_eq!(revalidated.revalidations_acknowledged, 1);
        assert_eq!(revalidated.downward_recoveries, 0);
    }

    #[test]
    fn failed_revalidation_reopens_from_base_and_preserves_packet_identity() {
        let mut state = PathMtuState::new(548, 1_472).expect("valid bounds");
        let completed_at = converge_clean_path(&mut state, 0);
        let previous_floor = state.ordinary_datagram_bytes();
        let deadline = completed_at + PATH_MTU_REVALIDATION_INTERVAL_MICROS;
        ready(&mut state, deadline);
        let probe = state
            .begin_probe(sequence(u16::MAX), previous_floor, deadline, 10_000)
            .expect("begin revalidation");
        let outcome = state.on_probe_loss(
            sequence(u16::MAX),
            MtuProbeFailure::SolePacketTimeout,
            deadline + 100,
            Some(100),
        );
        assert_eq!(probe.kind, MtuProbeKind::Revalidation);
        assert!(matches!(
            outcome,
            MtuProbeOutcome::LoweredCeiling {
                previous_floor: observed_previous,
                floor: 548,
                ceiling,
                retransmit_without_fragmentation_protection: retry,
                isolated_from_congestion: true,
                search_complete: false,
                ..
            } if observed_previous == previous_floor
                && ceiling == previous_floor - 1
                && retry == probe
        ));
        let reduced = state.snapshot();
        assert_eq!(reduced.phase, PathMtuPhase::Search);
        assert_eq!(reduced.floor_datagram_bytes, 548);
        assert_eq!(reduced.revalidations_failed, 1);
        assert_eq!(reduced.downward_recoveries, 1);
        assert!(state.retransmit_without_fragmentation_protection(sequence(u16::MAX)));
        assert_eq!(
            state.on_acknowledged(sequence(u16::MAX), deadline + 200, Some(100)),
            MtuProbeOutcome::RetryAcknowledged
        );
    }

    #[test]
    fn fixed_profile_never_revalidates_and_deadlines_saturate() {
        let mut fixed = PathMtuState::new(548, 548).expect("fixed bounds");
        ready(&mut fixed, 0);
        assert_eq!(fixed.snapshot().phase, PathMtuPhase::SearchComplete);
        assert_eq!(fixed.snapshot().revalidation_deadline_micros, None);
        assert!(!fixed.probe_ready(u64::MAX, 10_000));

        let mut dynamic = PathMtuState::new(548, 560).expect("narrow dynamic bounds");
        let near_wrap = u64::MAX - PATH_MTU_REVALIDATION_INTERVAL_MICROS / 2;
        ready(&mut dynamic, near_wrap);
        assert_eq!(
            dynamic.snapshot().revalidation_deadline_micros,
            Some(u64::MAX)
        );
        assert!(!dynamic.probe_ready(u64::MAX - 1, 10_000));
        assert!(dynamic.probe_ready(u64::MAX, 10_000));
    }
}

//! Deterministic bounded datagram-link fixtures for uTP state tests.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

pub(super) const MAX_SIM_EVENTS: usize = 131_072;
pub(super) const MAX_SIM_EVENT_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_SIM_QUEUE_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_JITTER_SAMPLES: usize = 64;
pub(super) const MAX_CLOCK_DRIFT_PPM: i32 = 100_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Direction {
    AToB,
    BToA,
}

impl Direction {
    const fn index(self) -> usize {
        match self {
            Self::AToB => 0,
            Self::BToA => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct EndpointClock {
    offset_micros: i64,
    drift_parts_per_million: i32,
}

impl EndpointClock {
    pub(super) fn new(
        offset_micros: i64,
        drift_parts_per_million: i32,
    ) -> Result<Self, SimulationError> {
        if drift_parts_per_million.unsigned_abs() > MAX_CLOCK_DRIFT_PPM as u32 {
            return Err(SimulationError::ClockDriftLimit {
                actual_parts_per_million: drift_parts_per_million,
                maximum_parts_per_million: MAX_CLOCK_DRIFT_PPM,
            });
        }
        Ok(Self {
            offset_micros,
            drift_parts_per_million,
        })
    }

    pub(super) fn timestamp(self, monotonic_micros: u64) -> u32 {
        let monotonic = i128::from(monotonic_micros);
        let drift = monotonic * i128::from(self.drift_parts_per_million) / 1_000_000;
        let local = monotonic + i128::from(self.offset_micros) + drift;
        local.rem_euclid(1_i128 << 32) as u32
    }

    pub(super) fn observed_one_way_delay(
        self,
        sender: Self,
        sent_at_micros: u64,
        received_at_micros: u64,
    ) -> u32 {
        self.timestamp(received_at_micros)
            .wrapping_sub(sender.timestamp(sent_at_micros))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LinkConfig {
    pub base_delay_micros: u64,
    pub jitter_pattern_micros: Vec<i64>,
    pub bytes_per_second: u64,
    pub queue_capacity_bytes: usize,
    pub path_udp_payload_mtu: usize,
    pub df_black_hole: bool,
}

impl LinkConfig {
    pub(super) fn validate(&self) -> Result<(), SimulationError> {
        if self.jitter_pattern_micros.len() > MAX_JITTER_SAMPLES {
            return Err(SimulationError::JitterSampleLimit {
                count: self.jitter_pattern_micros.len(),
                maximum: MAX_JITTER_SAMPLES,
            });
        }
        if self.bytes_per_second == 0 {
            return Err(SimulationError::ZeroBandwidth);
        }
        if self.queue_capacity_bytes > MAX_SIM_QUEUE_BYTES {
            return Err(SimulationError::QueueCapacityLimit {
                bytes: self.queue_capacity_bytes,
                maximum: MAX_SIM_QUEUE_BYTES,
            });
        }
        if !(150..=65_535).contains(&self.path_udp_payload_mtu) {
            return Err(SimulationError::InvalidMtu {
                bytes: self.path_udp_payload_mtu,
            });
        }
        for jitter in &self.jitter_pattern_micros {
            let minimum = -i128::from(self.base_delay_micros);
            if i128::from(*jitter) < minimum {
                return Err(SimulationError::NegativePropagationDelay {
                    base_micros: self.base_delay_micros,
                    jitter_micros: *jitter,
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ImpairmentScript {
    pub drop_ordinals: BTreeSet<u64>,
    pub duplicate_ordinals: BTreeSet<u64>,
    pub reorder_extra_micros: BTreeMap<u64, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SimDatagram {
    pub flow_id: u16,
    pub tag: u64,
    pub bytes: Vec<u8>,
    pub dont_fragment: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DeliveredDatagram {
    pub ordinal: u64,
    pub direction: Direction,
    pub sent_at_micros: u64,
    pub delivered_at_micros: u64,
    pub datagram: SimDatagram,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct LinkSnapshot {
    pub sent_datagrams: u64,
    pub sent_bytes: u64,
    pub delivered_datagrams: u64,
    pub delivered_bytes: u64,
    pub scripted_drops: u64,
    pub queue_drops: u64,
    pub mtu_black_hole_drops: u64,
    pub duplicates: u64,
    pub reordered: u64,
    pub pending_events: usize,
    pub pending_event_bytes: usize,
    pub event_high_water: usize,
    pub event_byte_high_water: usize,
    pub queue_bytes: usize,
    pub queue_byte_high_water: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SimulationError {
    JitterSampleLimit {
        count: usize,
        maximum: usize,
    },
    ZeroBandwidth,
    QueueCapacityLimit {
        bytes: usize,
        maximum: usize,
    },
    InvalidMtu {
        bytes: usize,
    },
    NegativePropagationDelay {
        base_micros: u64,
        jitter_micros: i64,
    },
    ClockDriftLimit {
        actual_parts_per_million: i32,
        maximum_parts_per_million: i32,
    },
    EmptyDatagram,
    DatagramTooLarge {
        bytes: usize,
        maximum: usize,
    },
    EventLimit {
        count: usize,
        maximum: usize,
    },
    EventByteLimit {
        bytes: usize,
        maximum: usize,
    },
    TimeReversed {
        previous_micros: u64,
        actual_micros: u64,
    },
}

impl fmt::Display for SimulationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JitterSampleLimit { count, maximum } => {
                write!(
                    formatter,
                    "simulation jitter samples {count} exceed {maximum}"
                )
            }
            Self::ZeroBandwidth => formatter.write_str("simulation bandwidth must be nonzero"),
            Self::QueueCapacityLimit { bytes, maximum } => {
                write!(
                    formatter,
                    "simulation queue capacity {bytes} exceeds {maximum}"
                )
            }
            Self::InvalidMtu { bytes } => {
                write!(
                    formatter,
                    "simulation UDP payload MTU {bytes} is outside 150..=65535"
                )
            }
            Self::NegativePropagationDelay {
                base_micros,
                jitter_micros,
            } => write!(
                formatter,
                "simulation base delay {base_micros} plus jitter {jitter_micros} is negative"
            ),
            Self::ClockDriftLimit {
                actual_parts_per_million,
                maximum_parts_per_million,
            } => write!(
                formatter,
                "simulation clock drift {actual_parts_per_million} ppm exceeds +/-{maximum_parts_per_million} ppm"
            ),
            Self::EmptyDatagram => formatter.write_str("simulation datagram is empty"),
            Self::DatagramTooLarge { bytes, maximum } => {
                write!(formatter, "simulation datagram {bytes} exceeds {maximum}")
            }
            Self::EventLimit { count, maximum } => {
                write!(
                    formatter,
                    "simulation pending events {count} exceed {maximum}"
                )
            }
            Self::EventByteLimit { bytes, maximum } => {
                write!(
                    formatter,
                    "simulation pending event bytes {bytes} exceed {maximum}"
                )
            }
            Self::TimeReversed {
                previous_micros,
                actual_micros,
            } => write!(
                formatter,
                "simulation time moved backward from {previous_micros} to {actual_micros}"
            ),
        }
    }
}

impl Error for SimulationError {}

#[derive(Clone, Debug)]
struct QueuedSerialization {
    departure_micros: u64,
    bytes: usize,
}

#[derive(Clone, Debug)]
struct DirectionState {
    next_departure_micros: u64,
    serialization_queue: VecDeque<QueuedSerialization>,
    queue_bytes: usize,
}

impl DirectionState {
    fn new() -> Self {
        Self {
            next_departure_micros: 0,
            serialization_queue: VecDeque::new(),
            queue_bytes: 0,
        }
    }

    fn advance_to(&mut self, now_micros: u64) {
        while self
            .serialization_queue
            .front()
            .is_some_and(|queued| queued.departure_micros <= now_micros)
        {
            let queued = self
                .serialization_queue
                .pop_front()
                .expect("front was present");
            self.queue_bytes -= queued.bytes;
        }
    }
}

#[derive(Clone, Debug)]
struct PendingDelivery {
    ordinal: u64,
    direction: Direction,
    sent_at_micros: u64,
    delivered_at_micros: u64,
    datagram: SimDatagram,
}

#[derive(Clone, Debug)]
pub(super) struct DeterministicLink {
    config: LinkConfig,
    script: ImpairmentScript,
    directions: [DirectionState; 2],
    events: BTreeMap<(u64, u64, u8), PendingDelivery>,
    now_micros: u64,
    next_ordinal: u64,
    pending_event_bytes: usize,
    snapshot: LinkSnapshot,
}

impl DeterministicLink {
    pub(super) fn new(
        config: LinkConfig,
        script: ImpairmentScript,
    ) -> Result<Self, SimulationError> {
        config.validate()?;
        Ok(Self {
            config,
            script,
            directions: [DirectionState::new(), DirectionState::new()],
            events: BTreeMap::new(),
            now_micros: 0,
            next_ordinal: 0,
            pending_event_bytes: 0,
            snapshot: LinkSnapshot::default(),
        })
    }

    pub(super) fn send(
        &mut self,
        direction: Direction,
        now_micros: u64,
        datagram: SimDatagram,
    ) -> Result<u64, SimulationError> {
        if datagram.bytes.is_empty() {
            return Err(SimulationError::EmptyDatagram);
        }
        if datagram.bytes.len() > 65_535 {
            return Err(SimulationError::DatagramTooLarge {
                bytes: datagram.bytes.len(),
                maximum: 65_535,
            });
        }
        self.advance_clock(now_micros)?;

        let ordinal = self.next_ordinal;
        let direction_index = direction.index();
        let next_queue_bytes = {
            let state = &mut self.directions[direction_index];
            state.advance_to(now_micros);
            state.queue_bytes.saturating_add(datagram.bytes.len())
        };
        self.refresh_queue_snapshot();
        if next_queue_bytes > self.config.queue_capacity_bytes {
            self.next_ordinal = self.next_ordinal.saturating_add(1);
            self.snapshot.sent_datagrams = self.snapshot.sent_datagrams.saturating_add(1);
            self.snapshot.sent_bytes = self
                .snapshot
                .sent_bytes
                .saturating_add(datagram.bytes.len() as u64);
            self.snapshot.queue_drops = self.snapshot.queue_drops.saturating_add(1);
            self.refresh_queue_snapshot();
            return Ok(ordinal);
        }

        let scripted_drop = self.script.drop_ordinals.contains(&ordinal);
        let mtu_black_hole = self.config.df_black_hole
            && datagram.dont_fragment
            && datagram.bytes.len() > self.config.path_udp_payload_mtu;
        let delivery_copies = if scripted_drop || mtu_black_hole {
            0
        } else if self.script.duplicate_ordinals.contains(&ordinal) {
            2
        } else {
            1
        };
        self.ensure_event_capacity(
            delivery_copies,
            datagram.bytes.len().saturating_mul(delivery_copies),
        )?;

        let state = &mut self.directions[direction_index];
        let serialization_micros = (datagram.bytes.len() as u64)
            .saturating_mul(1_000_000)
            .div_ceil(self.config.bytes_per_second)
            .max(1);
        let serialization_start = state.next_departure_micros.max(now_micros);
        let departure_micros = serialization_start.saturating_add(serialization_micros);
        state.next_departure_micros = departure_micros;
        state.serialization_queue.push_back(QueuedSerialization {
            departure_micros,
            bytes: datagram.bytes.len(),
        });
        state.queue_bytes = next_queue_bytes;
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        self.snapshot.sent_datagrams = self.snapshot.sent_datagrams.saturating_add(1);
        self.snapshot.sent_bytes = self
            .snapshot
            .sent_bytes
            .saturating_add(datagram.bytes.len() as u64);
        self.refresh_queue_snapshot();

        if scripted_drop {
            self.snapshot.scripted_drops = self.snapshot.scripted_drops.saturating_add(1);
            return Ok(ordinal);
        }
        if mtu_black_hole {
            self.snapshot.mtu_black_hole_drops =
                self.snapshot.mtu_black_hole_drops.saturating_add(1);
            return Ok(ordinal);
        }

        let jitter = if self.config.jitter_pattern_micros.is_empty() {
            0
        } else {
            self.config.jitter_pattern_micros
                [ordinal as usize % self.config.jitter_pattern_micros.len()]
        };
        let propagation = i128::from(self.config.base_delay_micros) + i128::from(jitter);
        let reordered_by = self
            .script
            .reorder_extra_micros
            .get(&ordinal)
            .copied()
            .unwrap_or(0);
        if reordered_by > 0 {
            self.snapshot.reordered = self.snapshot.reordered.saturating_add(1);
        }
        let delivered_at_micros = departure_micros
            .saturating_add(propagation as u64)
            .saturating_add(reordered_by);
        self.insert_delivery(
            (delivered_at_micros, ordinal, 0),
            PendingDelivery {
                ordinal,
                direction,
                sent_at_micros: now_micros,
                delivered_at_micros,
                datagram: datagram.clone(),
            },
        )?;

        if self.script.duplicate_ordinals.contains(&ordinal) {
            self.snapshot.duplicates = self.snapshot.duplicates.saturating_add(1);
            let duplicate_at = delivered_at_micros.saturating_add(1);
            self.insert_delivery(
                (duplicate_at, ordinal, 1),
                PendingDelivery {
                    ordinal,
                    direction,
                    sent_at_micros: now_micros,
                    delivered_at_micros: duplicate_at,
                    datagram,
                },
            )?;
        }
        Ok(ordinal)
    }

    pub(super) fn advance_to(
        &mut self,
        now_micros: u64,
    ) -> Result<Vec<DeliveredDatagram>, SimulationError> {
        self.advance_clock(now_micros)?;
        for direction in &mut self.directions {
            direction.advance_to(now_micros);
        }
        self.refresh_queue_snapshot();

        let split_key = (now_micros.saturating_add(1), 0, 0);
        let pending = self.events.split_off(&split_key);
        let ready = std::mem::replace(&mut self.events, pending);
        let mut delivered = Vec::with_capacity(ready.len());
        for (_, event) in ready {
            self.pending_event_bytes -= event.datagram.bytes.len();
            self.snapshot.delivered_datagrams = self.snapshot.delivered_datagrams.saturating_add(1);
            self.snapshot.delivered_bytes = self
                .snapshot
                .delivered_bytes
                .saturating_add(event.datagram.bytes.len() as u64);
            delivered.push(DeliveredDatagram {
                ordinal: event.ordinal,
                direction: event.direction,
                sent_at_micros: event.sent_at_micros,
                delivered_at_micros: event.delivered_at_micros,
                datagram: event.datagram,
            });
        }
        self.refresh_event_snapshot();
        Ok(delivered)
    }

    pub(super) fn next_delivery_micros(&self) -> Option<u64> {
        self.events.keys().next().map(|key| key.0)
    }

    pub(super) fn snapshot(&self) -> LinkSnapshot {
        self.snapshot
    }

    fn advance_clock(&mut self, now_micros: u64) -> Result<(), SimulationError> {
        if now_micros < self.now_micros {
            return Err(SimulationError::TimeReversed {
                previous_micros: self.now_micros,
                actual_micros: now_micros,
            });
        }
        self.now_micros = now_micros;
        Ok(())
    }

    fn insert_delivery(
        &mut self,
        key: (u64, u64, u8),
        event: PendingDelivery,
    ) -> Result<(), SimulationError> {
        self.ensure_event_capacity(1, event.datagram.bytes.len())?;
        let next_bytes = self.pending_event_bytes + event.datagram.bytes.len();
        let previous = self.events.insert(key, event);
        debug_assert!(previous.is_none(), "delivery key must be unique");
        self.pending_event_bytes = next_bytes;
        self.refresh_event_snapshot();
        Ok(())
    }

    fn ensure_event_capacity(
        &self,
        additional_events: usize,
        additional_bytes: usize,
    ) -> Result<(), SimulationError> {
        let next_count = self.events.len().checked_add(additional_events).ok_or(
            SimulationError::EventLimit {
                count: usize::MAX,
                maximum: MAX_SIM_EVENTS,
            },
        )?;
        if next_count > MAX_SIM_EVENTS {
            return Err(SimulationError::EventLimit {
                count: next_count,
                maximum: MAX_SIM_EVENTS,
            });
        }
        let next_bytes = self
            .pending_event_bytes
            .checked_add(additional_bytes)
            .ok_or(SimulationError::EventByteLimit {
                bytes: usize::MAX,
                maximum: MAX_SIM_EVENT_BYTES,
            })?;
        if next_bytes > MAX_SIM_EVENT_BYTES {
            return Err(SimulationError::EventByteLimit {
                bytes: next_bytes,
                maximum: MAX_SIM_EVENT_BYTES,
            });
        }
        Ok(())
    }

    fn refresh_event_snapshot(&mut self) {
        self.snapshot.pending_events = self.events.len();
        self.snapshot.pending_event_bytes = self.pending_event_bytes;
        self.snapshot.event_high_water = self.snapshot.event_high_water.max(self.events.len());
        self.snapshot.event_byte_high_water = self
            .snapshot
            .event_byte_high_water
            .max(self.pending_event_bytes);
    }

    fn refresh_queue_snapshot(&mut self) {
        let queue_bytes = self.directions.iter().map(|state| state.queue_bytes).sum();
        self.snapshot.queue_bytes = queue_bytes;
        self.snapshot.queue_byte_high_water = self.snapshot.queue_byte_high_water.max(queue_bytes);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TcpLikeSnapshot {
    pub congestion_window_bytes: usize,
    pub loss_reductions: u64,
}

#[derive(Clone, Debug)]
pub(super) struct TcpLikeSender {
    mss_bytes: usize,
    congestion_window_bytes: usize,
    next_loss_reduction_micros: u64,
    loss_reductions: u64,
}

impl TcpLikeSender {
    pub(super) fn new(mss_bytes: usize) -> Self {
        assert!(mss_bytes > 0, "TCP-like fixture MSS must be nonzero");
        Self {
            mss_bytes,
            congestion_window_bytes: mss_bytes.saturating_mul(2),
            next_loss_reduction_micros: 0,
            loss_reductions: 0,
        }
    }

    pub(super) fn on_ack(&mut self, acknowledged_bytes: usize) {
        let increase = acknowledged_bytes
            .saturating_mul(self.mss_bytes)
            .checked_div(self.congestion_window_bytes)
            .unwrap_or(0)
            .max(1);
        self.congestion_window_bytes = self.congestion_window_bytes.saturating_add(increase);
    }

    pub(super) fn on_loss(&mut self, now_micros: u64, round_trip_micros: u64) {
        if now_micros < self.next_loss_reduction_micros {
            return;
        }
        self.congestion_window_bytes =
            (self.congestion_window_bytes / 2).max(self.mss_bytes.saturating_mul(2));
        self.next_loss_reduction_micros = now_micros.saturating_add(round_trip_micros.max(1));
        self.loss_reductions = self.loss_reductions.saturating_add(1);
    }

    pub(super) fn snapshot(&self) -> TcpLikeSnapshot {
        TcpLikeSnapshot {
            congestion_window_bytes: self.congestion_window_bytes,
            loss_reductions: self.loss_reductions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LinkConfig {
        LinkConfig {
            base_delay_micros: 10_000,
            jitter_pattern_micros: vec![0],
            bytes_per_second: 1_000_000,
            queue_capacity_bytes: 4_000,
            path_udp_payload_mtu: 1_200,
            df_black_hole: true,
        }
    }

    fn datagram(tag: u64, bytes: usize, dont_fragment: bool) -> SimDatagram {
        SimDatagram {
            flow_id: 1,
            tag,
            bytes: vec![tag as u8; bytes],
            dont_fragment,
        }
    }

    #[test]
    fn serializes_shared_direction_and_preserves_opposite_direction_independence() {
        let mut link =
            DeterministicLink::new(config(), ImpairmentScript::default()).expect("valid link");
        link.send(Direction::AToB, 0, datagram(1, 1_000, false))
            .expect("first send");
        link.send(Direction::AToB, 0, datagram(2, 1_000, false))
            .expect("second send");
        link.send(Direction::BToA, 0, datagram(3, 1_000, false))
            .expect("reverse send");

        assert_eq!(link.next_delivery_micros(), Some(11_000));
        let first = link.advance_to(11_000).expect("advance");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].datagram.tag, 1);
        assert_eq!(first[1].datagram.tag, 3);
        assert_eq!(link.advance_to(12_000).expect("advance")[0].datagram.tag, 2);
        let snapshot = link.snapshot();
        assert_eq!(snapshot.delivered_datagrams, 3);
        assert_eq!(snapshot.queue_byte_high_water, 3_000);
        assert_eq!(snapshot.pending_events, 0);
        assert_eq!(snapshot.pending_event_bytes, 0);
    }

    #[test]
    fn scripted_loss_duplication_reordering_and_jitter_are_exact() {
        let mut link_config = config();
        link_config.jitter_pattern_micros = vec![-1_000, 2_000, 0, 0];
        let mut script = ImpairmentScript::default();
        script.drop_ordinals.insert(0);
        script.duplicate_ordinals.insert(1);
        script.reorder_extra_micros.insert(2, 20_000);
        let mut link = DeterministicLink::new(link_config, script).expect("valid link");
        for tag in 0..4 {
            link.send(Direction::AToB, 0, datagram(tag, 100, false))
                .expect("send");
        }

        let delivered = link.advance_to(40_000).expect("advance");
        let tags: Vec<_> = delivered
            .iter()
            .map(|delivery| delivery.datagram.tag)
            .collect();
        assert_eq!(tags, vec![3, 1, 1, 2]);
        let snapshot = link.snapshot();
        assert_eq!(snapshot.scripted_drops, 1);
        assert_eq!(snapshot.duplicates, 1);
        assert_eq!(snapshot.reordered, 1);
        assert_eq!(snapshot.delivered_datagrams, 4);
    }

    #[test]
    fn queue_pressure_and_df_black_hole_are_distinct() {
        let mut link_config = config();
        link_config.queue_capacity_bytes = 2_500;
        let mut link =
            DeterministicLink::new(link_config, ImpairmentScript::default()).expect("valid link");

        link.send(Direction::AToB, 0, datagram(1, 1_201, true))
            .expect("DF probe");
        link.send(Direction::AToB, 0, datagram(2, 1_201, false))
            .expect("fragmentable retry");
        link.send(Direction::AToB, 0, datagram(3, 100, false))
            .expect("queue overflow");

        let delivered = link.advance_to(20_000).expect("advance");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].datagram.tag, 2);
        let snapshot = link.snapshot();
        assert_eq!(snapshot.mtu_black_hole_drops, 1);
        assert_eq!(snapshot.queue_drops, 1);
        assert_eq!(snapshot.queue_byte_high_water, 2_402);
    }

    #[test]
    fn endpoint_clocks_preserve_offset_and_wrap_but_expose_drift() {
        let sender = EndpointClock::new(-1_000_000, 0).expect("sender clock");
        let receiver = EndpointClock::new(5_000_000, 0).expect("receiver clock");
        let before_wrap =
            receiver.observed_one_way_delay(sender, u32::MAX as u64 - 5, u32::MAX as u64 + 5);
        let after_wrap = receiver.observed_one_way_delay(sender, 10, 20);
        assert_eq!(before_wrap, after_wrap);
        assert_eq!(after_wrap, 6_000_010);

        let drifting = EndpointClock::new(5_000_000, 1_000).expect("drifting clock");
        assert_eq!(
            drifting.observed_one_way_delay(sender, 1_000_000, 1_010_000),
            6_011_010
        );
        assert!(matches!(
            EndpointClock::new(0, MAX_CLOCK_DRIFT_PPM + 1),
            Err(SimulationError::ClockDriftLimit { .. })
        ));
    }

    #[test]
    fn bounds_and_time_reversal_reject_without_partial_events() {
        let mut invalid = config();
        invalid.jitter_pattern_micros = vec![0; MAX_JITTER_SAMPLES + 1];
        assert!(matches!(
            DeterministicLink::new(invalid, ImpairmentScript::default()),
            Err(SimulationError::JitterSampleLimit { .. })
        ));

        let mut link =
            DeterministicLink::new(config(), ImpairmentScript::default()).expect("valid link");
        link.advance_to(100).expect("advance");
        let before = link.snapshot();
        assert!(matches!(
            link.send(Direction::AToB, 99, datagram(1, 100, false)),
            Err(SimulationError::TimeReversed { .. })
        ));
        assert_eq!(link.snapshot(), before);
    }

    #[test]
    fn tcp_like_competitor_has_reno_growth_and_one_loss_cut_per_rtt() {
        let mut sender = TcpLikeSender::new(1_000);
        assert_eq!(sender.snapshot().congestion_window_bytes, 2_000);
        sender.on_ack(2_000);
        assert_eq!(sender.snapshot().congestion_window_bytes, 3_000);
        sender.on_loss(10_000, 20_000);
        assert_eq!(sender.snapshot().congestion_window_bytes, 2_000);
        sender.on_loss(20_000, 20_000);
        assert_eq!(sender.snapshot().loss_reductions, 1);
        sender.on_loss(30_000, 20_000);
        assert_eq!(sender.snapshot().loss_reductions, 2);
    }
}

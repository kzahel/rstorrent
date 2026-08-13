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
        Self::with_initial_congestion_packets(mss_bytes, 2)
    }

    pub(super) fn with_initial_congestion_packets(
        mss_bytes: usize,
        initial_congestion_packets: usize,
    ) -> Self {
        assert!(mss_bytes > 0, "TCP-like fixture MSS must be nonzero");
        assert!(
            initial_congestion_packets > 0,
            "TCP-like fixture initial window must be nonzero"
        );
        Self {
            mss_bytes,
            congestion_window_bytes: mss_bytes.saturating_mul(initial_congestion_packets),
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
    use crate::utp::congestion::CongestionStartup;
    use crate::utp::{
        DatagramSendResult, IPV4_UDP_PAYLOAD_CEILING, IPV4_UDP_PAYLOAD_FLOOR, MAX_RECEIVE_BYTES,
        MAX_REORDER_PACKETS, MAX_SENT_BYTES, MAX_SENT_PACKETS, MAX_UNSENT_BYTES, SequenceNumber,
        TimestampMicros, TransportState, decode_packet,
    };
    use sha1::{Digest, Sha1};

    const TRANSFER_BYTES: usize = 4 * 1024 * 1024;
    const SIMULATION_TICK_MICROS: u64 = 250;
    const MAX_SCENARIO_MICROS: u64 = 300_000_000;
    type LossBatch = (u64, u16, Option<u16>, Vec<u16>, u64, u64);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ReceiveReleasePolicy {
        Immediate,
        HoldUntilFull { release_bytes: usize },
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct CompetitorConfig {
        starts_after_micros: u64,
        stops_after_micros: u64,
        segment_bytes: usize,
        initial_window_segments: usize,
        retransmit_after_micros: u64,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct TcpOutstanding {
        bytes: usize,
        sent_at_micros: u64,
    }

    #[derive(Clone, Debug)]
    struct TcpScenarioState {
        config: CompetitorConfig,
        sender: TcpLikeSender,
        outstanding: BTreeMap<u64, TcpOutstanding>,
        delivered_tags: BTreeSet<u64>,
        deliveries: Vec<(u64, usize, u64)>,
        next_tag: u64,
        stopped: bool,
        retransmissions: u64,
        congestion_window_high_water: usize,
    }

    impl TcpScenarioState {
        fn new(config: CompetitorConfig) -> Self {
            Self {
                config,
                sender: TcpLikeSender::with_initial_congestion_packets(
                    config.segment_bytes,
                    config.initial_window_segments,
                ),
                outstanding: BTreeMap::new(),
                delivered_tags: BTreeSet::new(),
                deliveries: Vec::new(),
                next_tag: 0,
                stopped: false,
                retransmissions: 0,
                congestion_window_high_water: config
                    .segment_bytes
                    .saturating_mul(config.initial_window_segments),
            }
        }

        fn outstanding_bytes(&self) -> usize {
            self.outstanding
                .values()
                .map(|outstanding| outstanding.bytes)
                .sum()
        }

        fn stop(&mut self) {
            self.stopped = true;
            self.outstanding.clear();
        }

        fn on_acknowledgement(&mut self, tag: u64) {
            if self.stopped {
                return;
            }
            if let Some(outstanding) = self.outstanding.remove(&tag) {
                self.sender.on_ack(outstanding.bytes);
                self.congestion_window_high_water = self
                    .congestion_window_high_water
                    .max(self.sender.snapshot().congestion_window_bytes);
            }
        }

        fn due_retransmission(&self, now_micros: u64) -> Option<u64> {
            (!self.stopped).then_some(())?;
            self.outstanding.iter().find_map(|(tag, outstanding)| {
                (now_micros.saturating_sub(outstanding.sent_at_micros)
                    >= self.config.retransmit_after_micros)
                    .then_some(*tag)
            })
        }
    }

    #[derive(Clone, Debug)]
    struct CompetitorReport {
        starts_at_micros: u64,
        stops_at_micros: u64,
        delivered_bytes: usize,
        deliveries: Vec<(u64, usize, u64)>,
        retransmissions: u64,
        loss_reductions: u64,
        congestion_window_high_water: usize,
        utp_rtt_at_stop_micros: u64,
    }

    impl CompetitorReport {
        fn overlap_share(&self, utp_deliveries: &[(u64, usize)]) -> f64 {
            let competitor_bytes: usize = self
                .deliveries
                .iter()
                .filter(|(at_micros, _, _)| {
                    *at_micros >= self.starts_at_micros && *at_micros < self.stops_at_micros
                })
                .map(|(_, bytes, _)| *bytes)
                .sum();
            let utp_bytes: usize = utp_deliveries
                .iter()
                .filter(|(at_micros, _)| {
                    *at_micros >= self.starts_at_micros && *at_micros < self.stops_at_micros
                })
                .map(|(_, bytes)| *bytes)
                .sum();
            competitor_bytes as f64 / competitor_bytes.saturating_add(utp_bytes).max(1) as f64
        }

        fn queue_delay_percentile(&self, percentile: usize) -> u64 {
            assert!((1..=100).contains(&percentile));
            let mut samples: Vec<_> = self
                .deliveries
                .iter()
                .filter(|(at_micros, _, _)| {
                    *at_micros >= self.starts_at_micros && *at_micros < self.stops_at_micros
                })
                .map(|(_, _, queue_delay)| *queue_delay)
                .collect();
            assert!(!samples.is_empty());
            samples.sort_unstable();
            let index = samples
                .len()
                .saturating_mul(percentile)
                .div_ceil(100)
                .saturating_sub(1);
            samples[index]
        }

        fn recovery_delay_micros(
            &self,
            utp_deliveries: &[(u64, usize)],
            bytes_per_second: u64,
        ) -> Option<u64> {
            let rtt = self.utp_rtt_at_stop_micros.max(1);
            let deadline = self.stops_at_micros.saturating_add(rtt.saturating_mul(10));
            utp_deliveries
                .iter()
                .map(|(at_micros, _)| *at_micros)
                .filter(|at_micros| {
                    *at_micros >= self.stops_at_micros.saturating_add(rtt) && *at_micros <= deadline
                })
                .find(|at_micros| {
                    let window_start = at_micros.saturating_sub(rtt);
                    let delivered: usize = utp_deliveries
                        .iter()
                        .filter(|(delivery_at, _)| {
                            *delivery_at > window_start && *delivery_at <= *at_micros
                        })
                        .map(|(_, bytes)| *bytes)
                        .sum();
                    delivered as u128 * 1_000_000
                        >= u128::from(bytes_per_second)
                            .saturating_mul(u128::from(rtt))
                            .saturating_mul(70)
                            / 100
                })
                .map(|at_micros| at_micros - self.stops_at_micros)
        }
    }

    #[derive(Clone, Debug)]
    struct UtpScenario {
        link: LinkConfig,
        script: ImpairmentScript,
        sender_clock: EndpointClock,
        receiver_clock: EndpointClock,
        transfer_bytes: usize,
        receive_release: ReceiveReleasePolicy,
        competitor: Option<CompetitorConfig>,
        sender_startup: CongestionStartup,
    }

    #[derive(Clone, Debug)]
    struct UtpScenarioReport {
        source_hash: [u8; 20],
        received_hash: [u8; 20],
        received_bytes: usize,
        started_at_micros: u64,
        completed_at_micros: u64,
        deliveries: Vec<(u64, usize)>,
        queue_delay_samples_micros: Vec<u32>,
        retransmissions: u64,
        maximum_transmissions_per_sequence: u8,
        mtu_probes: u64,
        loss_batches: Vec<LossBatch>,
        zero_receive_window_observed: bool,
        released_receive_window_bytes: Option<usize>,
        new_payload_emissions_while_remote_window_zero: u64,
        competitor: Option<CompetitorReport>,
        sender: crate::utp::TransportSnapshot,
        receiver: crate::utp::TransportSnapshot,
        link: LinkSnapshot,
        receive_packet_high_water: usize,
        receive_byte_high_water: usize,
        congestion_window_high_water_bytes: usize,
    }

    impl UtpScenarioReport {
        fn utilization_after(&self, warmup_micros: u64, bytes_per_second: u64) -> f64 {
            let measurement_start = self.started_at_micros.saturating_add(warmup_micros);
            let delivered_bytes: usize = self
                .deliveries
                .iter()
                .filter(|(at_micros, _)| *at_micros >= measurement_start)
                .map(|(_, bytes)| *bytes)
                .sum();
            let duration_micros = self
                .completed_at_micros
                .saturating_sub(measurement_start)
                .max(1);
            delivered_bytes as f64 * 1_000_000.0 / duration_micros as f64 / bytes_per_second as f64
        }

        fn queue_delay_percentile(&self, percentile: usize) -> u32 {
            assert!((1..=100).contains(&percentile));
            assert!(!self.queue_delay_samples_micros.is_empty());
            let mut samples = self.queue_delay_samples_micros.clone();
            samples.sort_unstable();
            let index = samples
                .len()
                .saturating_mul(percentile)
                .div_ceil(100)
                .saturating_sub(1);
            samples[index]
        }

        fn maximum_queue_delay(&self) -> u32 {
            self.queue_delay_samples_micros
                .iter()
                .copied()
                .max()
                .expect("scenario must observe acknowledged payload")
        }
    }

    fn default_utp_scenario() -> UtpScenario {
        UtpScenario {
            link: LinkConfig {
                base_delay_micros: 10_000,
                jitter_pattern_micros: vec![0],
                bytes_per_second: 500_000,
                queue_capacity_bytes: 75_000,
                path_udp_payload_mtu: IPV4_UDP_PAYLOAD_CEILING,
                df_black_hole: true,
            },
            script: ImpairmentScript::default(),
            sender_clock: EndpointClock::new(0, 0).expect("sender clock"),
            receiver_clock: EndpointClock::new(0, 0).expect("receiver clock"),
            transfer_bytes: TRANSFER_BYTES,
            receive_release: ReceiveReleasePolicy::Immediate,
            competitor: None,
            sender_startup: CongestionStartup::BoundedSlowStart,
        }
    }

    fn scenario_source(bytes: usize) -> Vec<u8> {
        (0..bytes)
            .map(|index| {
                let mixed = index.wrapping_mul(31) ^ index.rotate_left(7) ^ (index / 251);
                mixed as u8
            })
            .collect()
    }

    fn connected_transports(
        sender_clock: EndpointClock,
        receiver_clock: EndpointClock,
        sender_startup: CongestionStartup,
    ) -> (TransportState, TransportState, u64) {
        let mut sender = TransportState::initiate_for_diagnostics(
            40,
            SequenceNumber::new(10),
            0,
            IPV4_UDP_PAYLOAD_FLOOR,
            IPV4_UDP_PAYLOAD_CEILING,
            sender_startup,
        )
        .expect("initiate deterministic uTP connection");
        let syn = sender
            .poll_transmit(0, TimestampMicros::new(sender_clock.timestamp(0)))
            .expect("poll SYN")
            .expect("SYN emission");
        let syn_bytes = syn.encode().expect("encode SYN");
        sender
            .on_send_result(syn.intent.sequence_number, DatagramSendResult::Sent, 0)
            .expect("record SYN send");

        let mut receiver = TransportState::accept_syn(
            decode_packet(&syn_bytes).expect("decode SYN"),
            SequenceNumber::new(77),
            IPV4_UDP_PAYLOAD_FLOOR,
            IPV4_UDP_PAYLOAD_CEILING,
        )
        .expect("accept deterministic uTP connection");
        let state_at = 10_000;
        let state = receiver
            .poll_transmit(
                state_at,
                TimestampMicros::new(receiver_clock.timestamp(state_at)),
            )
            .expect("poll handshake STATE")
            .expect("handshake STATE emission");
        let state_bytes = state.encode().expect("encode handshake STATE");
        receiver
            .on_send_result(
                state.intent.sequence_number,
                DatagramSendResult::Sent,
                state_at,
            )
            .expect("record handshake STATE send");

        let connected_at = 20_000;
        sender
            .incoming(
                decode_packet(&state_bytes).expect("decode handshake STATE"),
                connected_at,
                TimestampMicros::new(sender_clock.timestamp(connected_at)),
            )
            .expect("complete deterministic uTP handshake");
        (sender, receiver, connected_at)
    }

    fn run_utp_scenario(scenario: UtpScenario) -> UtpScenarioReport {
        let source = scenario_source(scenario.transfer_bytes);
        let source_hash = Sha1::digest(&source).into();
        let (mut sender, mut receiver, started_at_micros) = connected_transports(
            scenario.sender_clock,
            scenario.receiver_clock,
            scenario.sender_startup,
        );
        let mut link = DeterministicLink::new(scenario.link.clone(), scenario.script)
            .expect("construct deterministic link");
        let mut competitor = scenario.competitor.map(TcpScenarioState::new);
        let mut competitor_rtt_at_stop_micros = None;
        let mut queued_source_bytes = 0;
        let mut received = Vec::with_capacity(source.len());
        let mut deliveries = Vec::new();
        let mut queue_delay_samples_micros = Vec::new();
        let mut retransmissions = 0_u64;
        let mut transmissions_by_sequence = BTreeMap::<SequenceNumber, u8>::new();
        let mut maximum_transmissions_per_sequence = 0;
        let mut mtu_probes = 0_u64;
        let mut loss_batches = Vec::new();
        let mut zero_receive_window_observed = false;
        let mut released_receive_window_bytes = None;
        let mut receive_pressure_released = false;
        let mut new_payload_emissions_while_remote_window_zero = 0_u64;
        let mut receive_packet_high_water = 0;
        let mut receive_byte_high_water = 0;
        let mut congestion_window_high_water_bytes =
            sender.snapshot().congestion.congestion_window_bytes;
        let deadline = started_at_micros.saturating_add(MAX_SCENARIO_MICROS);
        let mut now_micros = started_at_micros;

        loop {
            for delivery in link.advance_to(now_micros).expect("advance link") {
                match (delivery.datagram.flow_id, delivery.direction) {
                    (1, Direction::AToB) => {
                        let packet = decode_packet(&delivery.datagram.bytes)
                            .expect("decode uTP DATA datagram");
                        let outcome = receiver
                            .incoming(
                                packet,
                                now_micros,
                                TimestampMicros::new(scenario.receiver_clock.timestamp(now_micros)),
                            )
                            .expect("receive uTP datagram");
                        if let Some(receive) = outcome.connection.receive {
                            let mut consumed = 0;
                            for payload in receive.delivered {
                                consumed += payload.bytes.len();
                                deliveries.push((now_micros, payload.bytes.len()));
                                received.extend(payload.bytes);
                            }
                            if consumed > 0
                                && (scenario.receive_release == ReceiveReleasePolicy::Immediate
                                    || receive_pressure_released)
                            {
                                receiver
                                    .consume_received(consumed, now_micros)
                                    .expect("release delivered stream bytes");
                            }
                        }
                        if let ReceiveReleasePolicy::HoldUntilFull { release_bytes } =
                            scenario.receive_release
                            && !receive_pressure_released
                            && receiver
                                .snapshot()
                                .connection
                                .receive
                                .is_some_and(|receive| receive.advertised_window_bytes == 0)
                        {
                            zero_receive_window_observed = true;
                            released_receive_window_bytes = Some(
                                receiver
                                    .consume_received(release_bytes, now_micros)
                                    .expect("release exact receive-pressure credit"),
                            );
                            receive_pressure_released = true;
                        }
                        if let Some(receive) = receiver.snapshot().connection.receive {
                            receive_packet_high_water =
                                receive_packet_high_water.max(receive.packet_high_water);
                            receive_byte_high_water =
                                receive_byte_high_water.max(receive.byte_high_water);
                        }
                    }
                    (1, Direction::BToA) => {
                        let packet = decode_packet(&delivery.datagram.bytes)
                            .expect("decode uTP acknowledgement datagram");
                        let acknowledgement_number = packet.header.acknowledgement_number.get();
                        let before = sender.snapshot();
                        let outcome = sender
                            .incoming(
                                packet,
                                now_micros,
                                TimestampMicros::new(scenario.sender_clock.timestamp(now_micros)),
                            )
                            .expect("receive uTP acknowledgement");
                        let after = sender.snapshot();
                        if let Some(acknowledgement) = &outcome.connection.acknowledgement
                            && !acknowledgement.loss_signals.is_empty()
                        {
                            loss_batches.push((
                                now_micros,
                                acknowledgement_number,
                                before
                                    .mtu
                                    .active_probe
                                    .map(|probe| probe.sequence_number.get()),
                                acknowledgement
                                    .loss_signals
                                    .iter()
                                    .map(|sequence| sequence.get())
                                    .collect(),
                                after
                                    .congestion
                                    .loss_reductions
                                    .saturating_sub(before.congestion.loss_reductions),
                                after
                                    .congestion
                                    .ignored_loss_events
                                    .saturating_sub(before.congestion.ignored_loss_events),
                            ));
                        }
                        if outcome
                            .connection
                            .acknowledgement
                            .is_some_and(|acknowledgement| acknowledgement.acknowledged_bytes > 0)
                            && let Some(queue_delay) =
                                sender.snapshot().congestion.delay.queue_delay_micros
                        {
                            queue_delay_samples_micros.push(queue_delay);
                        }
                    }
                    (2, Direction::AToB) => {
                        let tcp = competitor.as_mut().expect("configured TCP-like flow");
                        if tcp.delivered_tags.insert(delivery.datagram.tag) {
                            let serialization_micros = (delivery.datagram.bytes.len() as u64)
                                .saturating_mul(1_000_000)
                                .div_ceil(scenario.link.bytes_per_second)
                                .max(1);
                            let queue_delay_micros = delivery
                                .delivered_at_micros
                                .saturating_sub(delivery.sent_at_micros)
                                .saturating_sub(scenario.link.base_delay_micros)
                                .saturating_sub(serialization_micros);
                            tcp.deliveries.push((
                                now_micros,
                                delivery.datagram.bytes.len(),
                                queue_delay_micros,
                            ));
                            link.send(
                                Direction::BToA,
                                now_micros,
                                SimDatagram {
                                    flow_id: 2,
                                    tag: delivery.datagram.tag,
                                    bytes: vec![0; 40],
                                    dont_fragment: false,
                                },
                            )
                            .expect("send TCP-like acknowledgement");
                        }
                    }
                    (2, Direction::BToA) => {
                        competitor
                            .as_mut()
                            .expect("configured TCP-like flow")
                            .on_acknowledgement(delivery.datagram.tag);
                    }
                    (flow_id, direction) => {
                        panic!("unexpected deterministic flow {flow_id} in {direction:?}")
                    }
                }
            }

            assert!(
                received.len() <= source.len(),
                "receiver delivered more bytes than were queued"
            );
            let sender_snapshot = sender.snapshot();
            let queue_capacity = MAX_UNSENT_BYTES - sender_snapshot.transmit.unsent_bytes;
            let source_remaining = source.len() - queued_source_bytes;
            let append_bytes = queue_capacity.min(source_remaining).min(64 * 1024);
            if append_bytes > 0 {
                sender
                    .queue_data(&source[queued_source_bytes..queued_source_bytes + append_bytes])
                    .expect("append bounded stream bytes");
                queued_source_bytes += append_bytes;
            }

            if let Some(tcp) = &mut competitor {
                let starts_at = started_at_micros.saturating_add(tcp.config.starts_after_micros);
                let stops_at = started_at_micros.saturating_add(tcp.config.stops_after_micros);
                if now_micros >= stops_at && !tcp.stopped {
                    competitor_rtt_at_stop_micros = Some(
                        sender
                            .snapshot()
                            .connection
                            .send
                            .rtt
                            .smoothed_rtt_micros
                            .unwrap_or(20_000),
                    );
                    tcp.stop();
                }
                if now_micros >= starts_at && now_micros < stops_at {
                    if let Some(tag) = tcp.due_retransmission(now_micros) {
                        tcp.sender.on_loss(now_micros, 20_000);
                        tcp.retransmissions = tcp.retransmissions.saturating_add(1);
                        tcp.outstanding
                            .get_mut(&tag)
                            .expect("due TCP-like packet remains outstanding")
                            .sent_at_micros = now_micros;
                        link.send(
                            Direction::AToB,
                            now_micros,
                            SimDatagram {
                                flow_id: 2,
                                tag,
                                bytes: vec![0; tcp.config.segment_bytes],
                                dont_fragment: false,
                            },
                        )
                        .expect("retransmit TCP-like segment");
                    }

                    while tcp.outstanding.len() < MAX_SENT_PACKETS
                        && tcp
                            .outstanding_bytes()
                            .saturating_add(tcp.config.segment_bytes)
                            <= tcp.sender.snapshot().congestion_window_bytes
                        && tcp
                            .outstanding_bytes()
                            .saturating_add(tcp.config.segment_bytes)
                            <= MAX_SENT_BYTES
                    {
                        let tag = tcp.next_tag;
                        tcp.next_tag = tcp.next_tag.saturating_add(1);
                        tcp.outstanding.insert(
                            tag,
                            TcpOutstanding {
                                bytes: tcp.config.segment_bytes,
                                sent_at_micros: now_micros,
                            },
                        );
                        link.send(
                            Direction::AToB,
                            now_micros,
                            SimDatagram {
                                flow_id: 2,
                                tag,
                                bytes: vec![0; tcp.config.segment_bytes],
                                dont_fragment: false,
                            },
                        )
                        .expect("send TCP-like segment");
                    }
                    tcp.congestion_window_high_water = tcp
                        .congestion_window_high_water
                        .max(tcp.sender.snapshot().congestion_window_bytes);
                }
            }

            if let Some(emission) = sender
                .poll_transmit(
                    now_micros,
                    TimestampMicros::new(scenario.sender_clock.timestamp(now_micros)),
                )
                .expect("poll sender")
            {
                if !emission.retransmission
                    && !emission.payload.is_empty()
                    && sender_snapshot.remote_window_bytes == 0
                {
                    new_payload_emissions_while_remote_window_zero =
                        new_payload_emissions_while_remote_window_zero.saturating_add(1);
                }
                let bytes = emission.encode().expect("encode sender datagram");
                retransmissions += u64::from(emission.retransmission);
                mtu_probes += u64::from(emission.mtu_probe);
                if !emission.payload.is_empty() {
                    let transmissions = transmissions_by_sequence
                        .entry(emission.intent.sequence_number)
                        .or_default();
                    *transmissions = transmissions.saturating_add(1);
                    maximum_transmissions_per_sequence =
                        maximum_transmissions_per_sequence.max(*transmissions);
                }
                sender
                    .on_send_result(
                        emission.intent.sequence_number,
                        DatagramSendResult::Sent,
                        now_micros,
                    )
                    .expect("record sender datagram");
                link.send(
                    Direction::AToB,
                    now_micros,
                    SimDatagram {
                        flow_id: 1,
                        tag: u64::from(emission.intent.sequence_number.get()),
                        bytes,
                        dont_fragment: emission.dont_fragment,
                    },
                )
                .expect("send through deterministic link");
            }

            if let Some(emission) = receiver
                .poll_transmit(
                    now_micros,
                    TimestampMicros::new(scenario.receiver_clock.timestamp(now_micros)),
                )
                .expect("poll receiver")
            {
                let bytes = emission.encode().expect("encode receiver datagram");
                receiver
                    .on_send_result(
                        emission.intent.sequence_number,
                        DatagramSendResult::Sent,
                        now_micros,
                    )
                    .expect("record receiver datagram");
                link.send(
                    Direction::BToA,
                    now_micros,
                    SimDatagram {
                        flow_id: 1,
                        tag: u64::from(emission.intent.sequence_number.get()),
                        bytes,
                        dont_fragment: emission.dont_fragment,
                    },
                )
                .expect("send acknowledgement through deterministic link");
            }

            let sender_snapshot = sender.snapshot();
            congestion_window_high_water_bytes = congestion_window_high_water_bytes
                .max(sender_snapshot.congestion.congestion_window_bytes);
            let mut receiver_snapshot = receiver.snapshot();
            if received.len() == source.len()
                && let ReceiveReleasePolicy::HoldUntilFull { .. } = scenario.receive_release
                && let Some(receive) = receiver_snapshot.connection.receive
                && receive.delivered_unconsumed_bytes > 0
            {
                receiver
                    .consume_received(receive.delivered_unconsumed_bytes, now_micros)
                    .expect("release retained receive-pressure bytes at completion");
                receiver_snapshot = receiver.snapshot();
            }
            let link_snapshot = link.snapshot();
            let complete = queued_source_bytes == source.len()
                && received.len() == source.len()
                && sender_snapshot.transmit.unsent_bytes == 0
                && sender_snapshot.connection.send.outstanding_bytes == 0
                && sender_snapshot.in_flight_bytes == 0
                && sender_snapshot.retransmissions.pending_packets == 0
                && sender_snapshot.pending_emission_bytes == 0
                && receiver_snapshot.acknowledgements.pending_packets == 0
                && receiver_snapshot.pending_emission_bytes == 0
                && competitor.as_ref().is_none_or(|tcp| tcp.stopped)
                && link_snapshot.pending_events == 0
                && link_snapshot.queue_bytes == 0;
            if complete {
                let received_hash = Sha1::digest(&received).into();
                let competitor = competitor.map(|tcp| CompetitorReport {
                    starts_at_micros: started_at_micros
                        .saturating_add(tcp.config.starts_after_micros),
                    stops_at_micros: started_at_micros
                        .saturating_add(tcp.config.stops_after_micros),
                    delivered_bytes: tcp.deliveries.iter().map(|(_, bytes, _)| *bytes).sum(),
                    deliveries: tcp.deliveries,
                    retransmissions: tcp.retransmissions,
                    loss_reductions: tcp.sender.snapshot().loss_reductions,
                    congestion_window_high_water: tcp.congestion_window_high_water,
                    utp_rtt_at_stop_micros: competitor_rtt_at_stop_micros.unwrap_or(20_000),
                });
                return UtpScenarioReport {
                    source_hash,
                    received_hash,
                    received_bytes: received.len(),
                    started_at_micros,
                    completed_at_micros: now_micros,
                    deliveries,
                    queue_delay_samples_micros,
                    retransmissions,
                    maximum_transmissions_per_sequence,
                    mtu_probes,
                    loss_batches,
                    zero_receive_window_observed,
                    released_receive_window_bytes,
                    new_payload_emissions_while_remote_window_zero,
                    competitor,
                    sender: sender_snapshot,
                    receiver: receiver_snapshot,
                    link: link_snapshot,
                    receive_packet_high_water,
                    receive_byte_high_water,
                    congestion_window_high_water_bytes,
                };
            }

            assert!(
                now_micros < deadline,
                "uTP scenario exceeded {} virtual microseconds: sender={sender_snapshot:?}, receiver={receiver_snapshot:?}, link={link_snapshot:?}, queued={queued_source_bytes}, received={}",
                MAX_SCENARIO_MICROS,
                received.len()
            );
            now_micros = now_micros.saturating_add(SIMULATION_TICK_MICROS);
        }
    }

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

    #[test]
    fn clean_four_mebibyte_utp_transfer_is_exact_bounded_and_saturates_link() {
        let scenario = default_utp_scenario();
        let bytes_per_second = scenario.link.bytes_per_second;
        let report = run_utp_scenario(scenario);

        assert_eq!(report.received_bytes, TRANSFER_BYTES);
        assert_eq!(report.received_hash, report.source_hash);
        assert_eq!(report.retransmissions, 0);
        assert_eq!(report.link.scripted_drops, 0);
        assert_eq!(report.link.queue_drops, 0);
        assert_eq!(report.link.mtu_black_hole_drops, 0);
        assert!(
            report.utilization_after(1_000_000, bytes_per_second) >= 0.8,
            "report={report:?}"
        );
        assert!(report.sender.transmit.byte_high_water <= MAX_UNSENT_BYTES);
        assert!(report.sender.connection.send.packet_high_water <= MAX_SENT_PACKETS);
        assert!(report.sender.connection.send.byte_high_water <= MAX_SENT_BYTES);
        assert!(report.receive_packet_high_water <= MAX_REORDER_PACKETS);
        assert!(report.receive_byte_high_water <= MAX_RECEIVE_BYTES);
        assert_eq!(report.sender.transmit.unsent_bytes, 0);
        assert_eq!(report.sender.connection.send.outstanding_bytes, 0);
        assert_eq!(
            report
                .receiver
                .connection
                .receive
                .unwrap()
                .total_buffered_bytes,
            0
        );
        assert_eq!(report.link.pending_event_bytes, 0);
        assert!(report.mtu_probes > 0);
        assert!(report.sender.mtu.search_complete);
        assert!(report.queue_delay_percentile(95) <= 150_000);
    }

    #[test]
    fn clean_long_rtt_high_capacity_fills_the_sender_window() {
        let mut scenario = default_utp_scenario();
        scenario.transfer_bytes = TRANSFER_BYTES * 2;
        scenario.link.base_delay_micros = 80_000;
        scenario.link.bytes_per_second = 12_500_000;
        scenario.link.queue_capacity_bytes = 2 * 1024 * 1024;
        let report = run_utp_scenario(scenario);
        let active_micros = report
            .completed_at_micros
            .saturating_sub(report.started_at_micros);

        assert_eq!(report.received_bytes, TRANSFER_BYTES * 2);
        assert_eq!(report.received_hash, report.source_hash);
        assert_eq!(report.retransmissions, 0, "report={report:?}");
        assert_eq!(report.link.queue_drops, 0, "report={report:?}");
        assert!(
            active_micros <= 19_000_000,
            "long-RTT transfer did not retain the pacing repair: report={report:?}"
        );
        assert!(
            report.sender.congestion.congestion_window_bytes > 128 * 1024,
            "long-RTT sender window did not grow: report={report:?}"
        );
        assert_eq!(report.sender.remote_window_limited_acknowledgements, 0);
        assert!(report.sender.window_growth_acknowledgements > 0);
        assert!(
            report.sender.congestion.slow_start_acknowledgements > 0,
            "sender never exercised bounded startup: report={report:?}"
        );
        assert!(
            report.sender.congestion.slow_start_active,
            "zero queue delay should not invent a startup exit: report={report:?}"
        );
        assert_eq!(report.sender.congestion.slow_start_exits, 0);
        assert_eq!(report.queue_delay_percentile(100), 0);
        assert!(
            report.sender.sender_underfilled_acknowledgements
                < report.sender.congestion_control_acknowledgements,
            "sender remained underfilled for every feedback event: report={report:?}"
        );
    }

    #[test]
    fn fixed_jitter_duplication_and_reordering_preserve_the_stream() {
        let mut scenario = default_utp_scenario();
        scenario.link.jitter_pattern_micros = vec![-3_000, 2_000, 0, 1_000, -1_000, 0];
        for ordinal in (137..20_000).step_by(137) {
            scenario.script.reorder_extra_micros.insert(ordinal, 12_000);
        }
        for ordinal in (311..20_000).step_by(311) {
            scenario.script.duplicate_ordinals.insert(ordinal);
        }
        let report = run_utp_scenario(scenario);

        assert_eq!(report.received_bytes, TRANSFER_BYTES);
        assert_eq!(report.received_hash, report.source_hash);
        assert!(report.link.reordered > 0);
        assert!(report.link.duplicates > 0);
        assert_eq!(report.link.scripted_drops, 0);
        assert_eq!(report.link.queue_drops, 0);
        assert!(report.receive_packet_high_water <= MAX_REORDER_PACKETS);
        assert!(report.receive_byte_high_water <= MAX_RECEIVE_BYTES);
        assert_ne!(
            report.sender.connection.phase,
            crate::utp::ConnectionPhase::Reset
        );
        assert_eq!(report.sender.connection.send.outstanding_bytes, 0);
        assert_eq!(
            report
                .receiver
                .connection
                .receive
                .unwrap()
                .total_buffered_bytes,
            0
        );
    }

    #[test]
    fn fixed_one_percent_noncongestive_loss_recovers_within_attempt_limit() {
        let mut scenario = default_utp_scenario();
        for ordinal in (99..20_000).step_by(100) {
            scenario.script.drop_ordinals.insert(ordinal);
        }
        let report = run_utp_scenario(scenario);

        assert_eq!(report.received_bytes, TRANSFER_BYTES);
        assert_eq!(report.received_hash, report.source_hash);
        assert!(report.link.scripted_drops > 0);
        assert_eq!(report.link.queue_drops, 0);
        assert!(report.retransmissions > 0);
        assert!(
            report.maximum_transmissions_per_sequence <= crate::utp::MAX_TRANSMISSIONS,
            "report={report:?}"
        );
        assert_eq!(report.sender.transmit.unsent_bytes, 0);
        assert_eq!(report.sender.connection.send.outstanding_bytes, 0);
        assert_eq!(report.sender.in_flight_bytes, 0);
        assert_eq!(report.sender.retransmissions.pending_packets, 0);
        assert_eq!(
            report
                .receiver
                .connection
                .receive
                .unwrap()
                .total_buffered_bytes,
            0
        );
        assert_eq!(report.link.pending_events, 0);
        assert_eq!(report.link.pending_event_bytes, 0);
        assert_eq!(report.link.queue_bytes, 0);
    }

    #[test]
    fn bounded_queue_reaches_target_without_persistent_bufferbloat() {
        let mut scenario = default_utp_scenario();
        scenario.link.queue_capacity_bytes = 55_000;
        let report = run_utp_scenario(scenario);

        assert_eq!(report.received_hash, report.source_hash);
        assert!(report.maximum_queue_delay() >= 80_000, "report={report:?}");
        assert!(
            report.queue_delay_percentile(95) <= 150_000,
            "report={report:?}"
        );
        assert!(report.link.queue_byte_high_water <= 55_000);
    }

    #[test]
    fn clock_offset_wrap_and_drift_keep_delay_control_bounded() {
        let mut scenario = default_utp_scenario();
        scenario.sender_clock =
            EndpointClock::new(i64::from(u32::MAX) - 1_000_000, -1_000).expect("sender clock");
        scenario.receiver_clock = EndpointClock::new(-3_000_000, 1_000).expect("receiver clock");
        let report = run_utp_scenario(scenario);

        assert_eq!(report.received_hash, report.source_hash);
        assert!(report.started_at_micros < u64::from(u32::MAX));
        assert!(report.completed_at_micros > 1_000_000);
        assert!(
            report.queue_delay_percentile(95) <= 150_000,
            "report={report:?}"
        );
        assert!(report.sender.congestion.congestion_window_bytes <= MAX_SENT_BYTES);
        assert_ne!(
            report.sender.connection.phase,
            crate::utp::ConnectionPhase::Reset
        );
    }

    #[test]
    fn df_black_hole_converges_mtu_without_congestion_reduction() {
        let mut scenario = default_utp_scenario();
        scenario.link.path_udp_payload_mtu = 1_280;
        let report = run_utp_scenario(scenario);

        assert_eq!(report.received_hash, report.source_hash);
        assert!(report.link.mtu_black_hole_drops > 0, "report={report:?}");
        assert!(report.retransmissions > 0);
        assert!(report.sender.mtu.search_complete, "report={report:?}");
        assert!(report.sender.mtu.probes_started <= 10, "report={report:?}");
        assert!(report.sender.mtu.probes_failed > 0);
        assert!(report.sender.mtu.floor_datagram_bytes <= 1_280);
        assert!(1_280 - report.sender.mtu.floor_datagram_bytes <= 16);
        assert_eq!(report.link.queue_drops, 0, "report={report:?}");
        assert_eq!(
            report.sender.congestion.loss_reductions, 0,
            "loss batches={:?}",
            report.loss_batches
        );
        assert_eq!(report.sender.congestion.timeout_collapses, 0);
        assert!(report.sender.congestion.ignored_loss_events >= report.sender.mtu.probes_failed);
    }

    #[test]
    fn receive_pressure_closes_and_reopens_exact_stream_credit() {
        let mut scenario = default_utp_scenario();
        scenario.transfer_bytes = MAX_RECEIVE_BYTES * 2;
        scenario.receive_release = ReceiveReleasePolicy::HoldUntilFull {
            release_bytes: 64 * 1024,
        };
        let report = run_utp_scenario(scenario);

        assert_eq!(report.received_bytes, MAX_RECEIVE_BYTES * 2);
        assert_eq!(report.received_hash, report.source_hash);
        assert!(report.zero_receive_window_observed, "report={report:?}");
        assert_eq!(report.released_receive_window_bytes, Some(64 * 1024));
        assert_eq!(report.new_payload_emissions_while_remote_window_zero, 0);
        assert_eq!(report.receive_byte_high_water, MAX_RECEIVE_BYTES);
        assert_eq!(
            report
                .receiver
                .connection
                .receive
                .unwrap()
                .total_buffered_bytes,
            0
        );
        assert_eq!(report.sender.connection.send.outstanding_bytes, 0);
    }

    #[test]
    fn tcp_like_foreground_dominates_then_utp_recovers_within_ten_rtts() {
        let mut scenario = default_utp_scenario();
        scenario.transfer_bytes = TRANSFER_BYTES * 2;
        scenario.competitor = Some(CompetitorConfig {
            starts_after_micros: 1_000_000,
            stops_after_micros: 6_000_000,
            segment_bytes: 1_000,
            initial_window_segments: 64,
            retransmit_after_micros: 200_000,
        });
        let bytes_per_second = scenario.link.bytes_per_second;
        let report = run_utp_scenario(scenario);
        let competitor = report.competitor.as_ref().expect("competitor report");

        assert_eq!(report.received_hash, report.source_hash);
        assert!(competitor.delivered_bytes > 0);
        let overlap_share = competitor.overlap_share(&report.deliveries);
        assert!(
            overlap_share >= 0.70,
            "competitor overlap share={overlap_share}, delivered={}, cwnd_high={}, losses={}",
            competitor.delivered_bytes,
            competitor.congestion_window_high_water,
            competitor.loss_reductions
        );
        let competitor_queue_p95 = competitor.queue_delay_percentile(95);
        assert!(competitor_queue_p95 <= 150_000, "report={report:?}");
        let recovery_delay = competitor
            .recovery_delay_micros(&report.deliveries, bytes_per_second)
            .unwrap_or_else(|| panic!("uTP did not recover within ten RTTs: report={report:?}"));
        assert!(
            recovery_delay <= competitor.utp_rtt_at_stop_micros * 10,
            "report={report:?}"
        );
        assert!(competitor.congestion_window_high_water >= 2_000);
        assert!(competitor.loss_reductions > 0);
        assert!(competitor.retransmissions >= competitor.loss_reductions);
        assert!(report.sender.congestion.loss_reductions > 0);
        assert!(report.link.queue_drops > 0);
        // The snapshot sums the independently bounded forward and reverse
        // queues; permit one maximum uTP acknowledgement datagram in reverse.
        assert!(
            report.link.queue_byte_high_water <= 75_000 + IPV4_UDP_PAYLOAD_CEILING,
            "report={report:?}"
        );
    }

    #[test]
    #[ignore = "Tactical 150 source-policy comparison"]
    fn exact_slow_start_is_rejected_by_queue_delay_gate() {
        let mut current = default_utp_scenario();
        current.transfer_bytes = TRANSFER_BYTES * 2;
        current.link.base_delay_micros = 80_000;
        current.link.bytes_per_second = 3_000_000;
        current.link.queue_capacity_bytes = 2 * 1024 * 1024;
        let current = run_utp_scenario(current);

        let mut exact = default_utp_scenario();
        exact.sender_startup = CongestionStartup::ExactSlowStart;
        exact.transfer_bytes = TRANSFER_BYTES * 2;
        exact.link.base_delay_micros = 80_000;
        exact.link.bytes_per_second = 3_000_000;
        exact.link.queue_capacity_bytes = 2 * 1024 * 1024;
        let exact = run_utp_scenario(exact);

        let current_duration = current
            .completed_at_micros
            .saturating_sub(current.started_at_micros);
        let exact_duration = exact
            .completed_at_micros
            .saturating_sub(exact.started_at_micros);
        eprintln!(
            "T150_EXACT_REJECTION current_duration_us={current_duration} exact_duration_us={exact_duration} exact_queue_p95_us={} exact_queue_max_us={} exact_cwnd_high={} exact_flight_high={}",
            exact.queue_delay_percentile(95),
            exact.maximum_queue_delay(),
            exact.congestion_window_high_water_bytes,
            exact.sender.in_flight_byte_high_water,
        );
        assert_eq!(exact.received_hash, exact.source_hash);
        assert!(exact_duration < current_duration);
        assert!(exact.queue_delay_percentile(95) > 150_000);
    }

    #[test]
    #[ignore = "Tactical 150 production-policy A/B"]
    fn bounded_slow_start_ab_covers_long_rtt_fairness_and_resources() {
        let base_delay_profiles_micros = [70_000, 80_000, 90_000];
        let mut paired_durations = Vec::new();

        for (sample, base_delay_micros) in base_delay_profiles_micros.into_iter().enumerate() {
            let mut durations = Vec::new();
            for startup in [
                CongestionStartup::LinearLedbat,
                CongestionStartup::BoundedSlowStart,
            ] {
                let mut scenario = default_utp_scenario();
                scenario.sender_startup = startup;
                scenario.transfer_bytes = TRANSFER_BYTES * 2;
                scenario.link.base_delay_micros = base_delay_micros;
                scenario.link.bytes_per_second = 3_000_000;
                scenario.link.queue_capacity_bytes = 2 * 1024 * 1024;
                let report = run_utp_scenario(scenario);
                let duration_micros = report
                    .completed_at_micros
                    .saturating_sub(report.started_at_micros);
                let rate_mib_per_second = report.received_bytes as f64 * 1_000_000.0
                    / duration_micros.max(1) as f64
                    / (1024.0 * 1024.0);
                let startup_name = match startup {
                    CongestionStartup::LinearLedbat => "linear",
                    CongestionStartup::BoundedSlowStart => "slow-start-10ms-30pct",
                    CongestionStartup::ExactSlowStart => "exact-slow-start",
                };
                eprintln!(
                    "T150_STARTUP sample={sample} mode={startup_name} duration_us={duration_micros} rate_mib_s={rate_mib_per_second:.6} cwnd_high={} flight_high={} queue_p95_us={} queue_max_us={} retransmissions={} loss_reductions={} timeout_collapses={} slow_start_acks={} slow_start_exits={} send_packet_high={} send_byte_high={} event_high={} event_byte_high={} queue_byte_high={}",
                    report.congestion_window_high_water_bytes,
                    report.sender.in_flight_byte_high_water,
                    report.queue_delay_percentile(95),
                    report.maximum_queue_delay(),
                    report.retransmissions,
                    report.sender.congestion.loss_reductions,
                    report.sender.congestion.timeout_collapses,
                    report.sender.congestion.slow_start_acknowledgements,
                    report.sender.congestion.slow_start_exits,
                    report.sender.connection.send.packet_high_water,
                    report.sender.connection.send.byte_high_water,
                    report.link.event_high_water,
                    report.link.event_byte_high_water,
                    report.link.queue_byte_high_water,
                );

                assert_eq!(report.received_hash, report.source_hash);
                assert_eq!(report.received_bytes, TRANSFER_BYTES * 2);
                assert_eq!(report.link.scripted_drops, 0);
                assert_eq!(report.link.queue_drops, 0);
                assert_eq!(report.link.mtu_black_hole_drops, 0);
                assert!(report.queue_delay_percentile(95) <= 150_000);
                assert!(report.sender.connection.send.packet_high_water <= MAX_SENT_PACKETS);
                assert!(report.sender.connection.send.byte_high_water <= MAX_SENT_BYTES);
                assert!(report.sender.in_flight_byte_high_water <= MAX_SENT_BYTES);
                assert!(report.link.event_high_water <= MAX_SIM_EVENTS);
                assert!(report.link.event_byte_high_water <= MAX_SIM_EVENT_BYTES);
                if startup == CongestionStartup::BoundedSlowStart {
                    assert!(report.sender.congestion.slow_start_acknowledgements > 0);
                    assert!(report.sender.congestion.slow_start_exits > 0);
                }
                durations.push(duration_micros);
            }
            assert!(
                durations[1] < durations[0],
                "slow-start candidate did not improve sample {sample}: {durations:?}"
            );
            paired_durations.push((durations[0], durations[1]));
        }

        for startup in [
            CongestionStartup::LinearLedbat,
            CongestionStartup::BoundedSlowStart,
        ] {
            let mut scenario = default_utp_scenario();
            scenario.sender_startup = startup;
            scenario.transfer_bytes = TRANSFER_BYTES * 2;
            scenario.competitor = Some(CompetitorConfig {
                starts_after_micros: 1_000_000,
                stops_after_micros: 6_000_000,
                segment_bytes: 1_000,
                initial_window_segments: 64,
                retransmit_after_micros: 200_000,
            });
            let bytes_per_second = scenario.link.bytes_per_second;
            let report = run_utp_scenario(scenario);
            let competitor = report.competitor.as_ref().expect("competitor report");
            let overlap_share = competitor.overlap_share(&report.deliveries);
            let queue_p95 = competitor.queue_delay_percentile(95);
            let recovery_delay = competitor
                .recovery_delay_micros(&report.deliveries, bytes_per_second)
                .expect("uTP recovery within ten RTTs");
            let startup_name = match startup {
                CongestionStartup::LinearLedbat => "linear",
                CongestionStartup::BoundedSlowStart => "slow-start-10ms-30pct",
                CongestionStartup::ExactSlowStart => "exact-slow-start",
            };
            eprintln!(
                "T150_FAIRNESS mode={startup_name} competitor_share={overlap_share:.6} competitor_queue_p95_us={queue_p95} utp_recovery_us={recovery_delay} utp_rtt_stop_us={} utp_loss_reductions={} queue_drops={} queue_byte_high={} cwnd_high={} flight_high={}",
                competitor.utp_rtt_at_stop_micros,
                report.sender.congestion.loss_reductions,
                report.link.queue_drops,
                report.link.queue_byte_high_water,
                report.congestion_window_high_water_bytes,
                report.sender.in_flight_byte_high_water,
            );
            assert_eq!(report.received_hash, report.source_hash);
            assert!(overlap_share >= 0.70);
            assert!(queue_p95 <= 150_000);
            assert!(recovery_delay <= competitor.utp_rtt_at_stop_micros * 10);
            assert!(report.link.queue_byte_high_water <= 75_000 + IPV4_UDP_PAYLOAD_CEILING);
        }

        let mut loss = default_utp_scenario();
        loss.sender_startup = CongestionStartup::BoundedSlowStart;
        loss.link.queue_capacity_bytes = 1024 * 1024;
        for ordinal in (99..20_000).step_by(100) {
            loss.script.drop_ordinals.insert(ordinal);
        }
        let loss = run_utp_scenario(loss);
        eprintln!(
            "T150_LOSS duration_us={} scripted_drops={} queue_drops={} retransmissions={} loss_reductions={} timeout_collapses={} max_transmissions={} cwnd_high={} flight_high={} send_packet_high={} send_byte_high={}",
            loss.completed_at_micros
                .saturating_sub(loss.started_at_micros),
            loss.link.scripted_drops,
            loss.link.queue_drops,
            loss.retransmissions,
            loss.sender.congestion.loss_reductions,
            loss.sender.congestion.timeout_collapses,
            loss.maximum_transmissions_per_sequence,
            loss.congestion_window_high_water_bytes,
            loss.sender.in_flight_byte_high_water,
            loss.sender.connection.send.packet_high_water,
            loss.sender.connection.send.byte_high_water,
        );
        assert_eq!(loss.received_hash, loss.source_hash);
        assert!(loss.link.scripted_drops > 0);
        assert_eq!(loss.link.queue_drops, 0);
        assert!(loss.retransmissions > 0);
        assert!(loss.sender.congestion.loss_reductions > 0);
        assert!(loss.maximum_transmissions_per_sequence <= crate::utp::MAX_TRANSMISSIONS);

        let mut mtu = default_utp_scenario();
        mtu.sender_startup = CongestionStartup::BoundedSlowStart;
        mtu.link.queue_capacity_bytes = 1024 * 1024;
        mtu.link.path_udp_payload_mtu = 1_280;
        let mtu = run_utp_scenario(mtu);
        eprintln!(
            "T150_MTU duration_us={} mtu_black_hole_drops={} retransmissions={} loss_reductions={} ignored_loss_events={} probes_started={} probes_failed={} floor_datagram_bytes={} cwnd_high={} flight_high={}",
            mtu.completed_at_micros
                .saturating_sub(mtu.started_at_micros),
            mtu.link.mtu_black_hole_drops,
            mtu.retransmissions,
            mtu.sender.congestion.loss_reductions,
            mtu.sender.congestion.ignored_loss_events,
            mtu.sender.mtu.probes_started,
            mtu.sender.mtu.probes_failed,
            mtu.sender.mtu.floor_datagram_bytes,
            mtu.congestion_window_high_water_bytes,
            mtu.sender.in_flight_byte_high_water,
        );
        assert_eq!(mtu.received_hash, mtu.source_hash);
        assert!(mtu.link.mtu_black_hole_drops > 0);
        assert!(mtu.sender.mtu.search_complete);
        assert_eq!(mtu.sender.congestion.loss_reductions, 0);
        assert!(mtu.sender.congestion.ignored_loss_events >= mtu.sender.mtu.probes_failed);

        eprintln!("T150_PAIRED_DURATIONS {paired_durations:?}");
    }
}

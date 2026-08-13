//! Runtime-independent RFC 6817 LEDBAT congestion and pacing state.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use super::{INITIAL_RTO_MICROS, MAX_UTP_PAYLOAD_SIZE};

pub const TARGET_DELAY_MICROS: u32 = 100_000;
pub const BASE_DELAY_BUCKETS: usize = 10;
pub const CURRENT_DELAY_SAMPLE_LIMIT: usize = 32;
pub const INITIAL_CONGESTION_PACKETS: usize = 2;
pub const MIN_CONGESTION_PACKETS: usize = 2;
pub const MAX_CONGESTION_WINDOW_BYTES: usize = 1024 * 1024;
pub const STARTUP_EXIT_DELAY_MICROS: u32 = 10_000;
pub const STARTUP_EXIT_WINDOW_PERCENT: u8 = 30;
const MICROS_PER_MINUTE: u64 = 60_000_000;
const FIXED_POINT_SHIFT: u32 = 16;
const FIXED_POINT_ONE: i128 = 1_i128 << FIXED_POINT_SHIFT;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DelaySnapshot {
    pub base_delay_micros: Option<u32>,
    pub current_delay_micros: Option<u32>,
    pub queue_delay_micros: Option<u32>,
    pub populated_base_buckets: usize,
    pub current_samples: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DelaySample {
    observed_at_micros: u64,
    delay_micros: u32,
}

#[derive(Clone, Debug)]
struct DelayHistory {
    base_buckets: [Option<u32>; BASE_DELAY_BUCKETS],
    current_bucket: usize,
    current_minute: Option<u64>,
    current_samples: VecDeque<DelaySample>,
    last_now_micros: Option<u64>,
}

impl DelayHistory {
    fn new() -> Self {
        Self {
            base_buckets: [None; BASE_DELAY_BUCKETS],
            current_bucket: 0,
            current_minute: None,
            current_samples: VecDeque::with_capacity(CURRENT_DELAY_SAMPLE_LIMIT),
            last_now_micros: None,
        }
    }

    fn add_sample(
        &mut self,
        now_micros: u64,
        delay_micros: u32,
        current_horizon_micros: u64,
    ) -> Result<DelaySnapshot, CongestionError> {
        self.advance_time(now_micros, current_horizon_micros)?;
        let bucket = &mut self.base_buckets[self.current_bucket];
        if bucket.is_none_or(|base| wrapping_delay_less(delay_micros, base)) {
            *bucket = Some(delay_micros);
        }
        if self.current_samples.len() == CURRENT_DELAY_SAMPLE_LIMIT {
            self.current_samples.pop_front();
        }
        self.current_samples.push_back(DelaySample {
            observed_at_micros: now_micros,
            delay_micros,
        });
        Ok(self.snapshot(None))
    }

    fn advance_time(
        &mut self,
        now_micros: u64,
        current_horizon_micros: u64,
    ) -> Result<(), CongestionError> {
        if let Some(previous_micros) = self.last_now_micros
            && now_micros < previous_micros
        {
            return Err(CongestionError::TimeReversed {
                previous_micros,
                actual_micros: now_micros,
            });
        }
        let minute = now_micros / MICROS_PER_MINUTE;
        match self.current_minute {
            None => self.current_minute = Some(minute),
            Some(previous_minute) if minute > previous_minute => {
                let elapsed = minute - previous_minute;
                if elapsed >= BASE_DELAY_BUCKETS as u64 {
                    self.base_buckets.fill(None);
                    self.current_bucket = 0;
                } else {
                    for _ in 0..elapsed {
                        self.current_bucket = (self.current_bucket + 1) % BASE_DELAY_BUCKETS;
                        self.base_buckets[self.current_bucket] = None;
                    }
                }
                self.current_minute = Some(minute);
            }
            Some(_) => {}
        }
        while self.current_samples.front().is_some_and(|sample| {
            now_micros.saturating_sub(sample.observed_at_micros) > current_horizon_micros
        }) {
            self.current_samples.pop_front();
        }
        self.last_now_micros = Some(now_micros);
        Ok(())
    }

    fn snapshot(&self, rtt_cap_micros: Option<u64>) -> DelaySnapshot {
        let base_delay_micros = wrapping_min(self.base_buckets.iter().flatten().copied());
        let current_delay_micros = base_delay_micros.and_then(|base| {
            self.current_samples
                .iter()
                .map(|sample| sample.delay_micros.wrapping_sub(base))
                .min()
        });
        let queue_delay_micros = current_delay_micros.map(|delay| {
            rtt_cap_micros.map_or(delay, |rtt| delay.min(rtt.min(u64::from(u32::MAX)) as u32))
        });
        DelaySnapshot {
            base_delay_micros,
            current_delay_micros,
            queue_delay_micros,
            populated_base_buckets: self.base_buckets.iter().flatten().count(),
            current_samples: self.current_samples.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CongestionSnapshot {
    pub maximum_segment_bytes: usize,
    pub congestion_window_bytes: usize,
    pub slow_start_active: bool,
    pub slow_start_threshold_bytes: Option<usize>,
    pub slow_start_acknowledgements: u64,
    pub slow_start_exits: u64,
    pub loss_reductions: u64,
    pub ignored_loss_events: u64,
    pub timeout_collapses: u64,
    pub timeout_collapsed: bool,
    pub next_loss_reduction_micros: u64,
    pub delay: DelaySnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CongestionStartup {
    BoundedSlowStart,
    #[cfg(test)]
    LinearLedbat,
    #[cfg(test)]
    ExactSlowStart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CongestionAckOutcome {
    pub previous_window_bytes: usize,
    pub congestion_window_bytes: usize,
    pub queue_delay_micros: u32,
    pub window_delta_bytes: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CongestionError {
    InvalidMaximumSegmentSize {
        bytes: usize,
        maximum: usize,
    },
    TimeReversed {
        previous_micros: u64,
        actual_micros: u64,
    },
}

impl fmt::Display for CongestionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMaximumSegmentSize { bytes, maximum } => write!(
                formatter,
                "uTP maximum segment size {bytes} is outside 1..={maximum}"
            ),
            Self::TimeReversed {
                previous_micros,
                actual_micros,
            } => write!(
                formatter,
                "uTP congestion time moved backward from {previous_micros} to {actual_micros}"
            ),
        }
    }
}

impl Error for CongestionError {}

#[derive(Clone, Debug)]
pub struct CongestionController {
    maximum_segment_bytes: usize,
    congestion_window_fixed: i128,
    slow_start_exit_delay_micros: Option<u32>,
    slow_start_exit_window_percent: Option<u8>,
    slow_start_active: bool,
    slow_start_threshold_bytes: Option<usize>,
    slow_start_acknowledgements: u64,
    slow_start_exits: u64,
    delay: DelayHistory,
    loss_reductions: u64,
    ignored_loss_events: u64,
    timeout_collapses: u64,
    timeout_collapsed: bool,
    next_loss_reduction_micros: u64,
}

impl CongestionController {
    pub fn new(maximum_segment_bytes: usize) -> Result<Self, CongestionError> {
        Self::with_startup(maximum_segment_bytes, CongestionStartup::BoundedSlowStart)
    }

    pub(super) fn with_startup(
        maximum_segment_bytes: usize,
        startup: CongestionStartup,
    ) -> Result<Self, CongestionError> {
        validate_mss(maximum_segment_bytes)?;
        let (slow_start_exit_delay_micros, exit_window_percent) = match startup {
            CongestionStartup::BoundedSlowStart => (
                Some(STARTUP_EXIT_DELAY_MICROS),
                Some(STARTUP_EXIT_WINDOW_PERCENT),
            ),
            #[cfg(test)]
            CongestionStartup::LinearLedbat => (None, None),
            #[cfg(test)]
            CongestionStartup::ExactSlowStart => (Some(TARGET_DELAY_MICROS), None),
        };
        Ok(Self {
            maximum_segment_bytes,
            congestion_window_fixed: bytes_to_fixed(
                maximum_segment_bytes.saturating_mul(INITIAL_CONGESTION_PACKETS),
            ),
            slow_start_exit_delay_micros,
            slow_start_exit_window_percent: exit_window_percent,
            slow_start_active: slow_start_exit_delay_micros.is_some(),
            slow_start_threshold_bytes: None,
            slow_start_acknowledgements: 0,
            slow_start_exits: 0,
            delay: DelayHistory::new(),
            loss_reductions: 0,
            ignored_loss_events: 0,
            timeout_collapses: 0,
            timeout_collapsed: false,
            next_loss_reduction_micros: 0,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> CongestionSnapshot {
        CongestionSnapshot {
            maximum_segment_bytes: self.maximum_segment_bytes,
            congestion_window_bytes: self.congestion_window_bytes(),
            slow_start_active: self.slow_start_active,
            slow_start_threshold_bytes: self.slow_start_threshold_bytes,
            slow_start_acknowledgements: self.slow_start_acknowledgements,
            slow_start_exits: self.slow_start_exits,
            loss_reductions: self.loss_reductions,
            ignored_loss_events: self.ignored_loss_events,
            timeout_collapses: self.timeout_collapses,
            timeout_collapsed: self.timeout_collapsed,
            next_loss_reduction_micros: self.next_loss_reduction_micros,
            delay: self.delay.snapshot(None),
        }
    }

    pub fn update_maximum_segment_size(
        &mut self,
        maximum_segment_bytes: usize,
    ) -> Result<(), CongestionError> {
        validate_mss(maximum_segment_bytes)?;
        self.maximum_segment_bytes = maximum_segment_bytes;
        let floor_packets = if self.timeout_collapsed {
            1
        } else {
            MIN_CONGESTION_PACKETS
        };
        let floor = bytes_to_fixed(maximum_segment_bytes.saturating_mul(floor_packets));
        self.congestion_window_fixed = self
            .congestion_window_fixed
            .max(floor)
            .min(bytes_to_fixed(MAX_CONGESTION_WINDOW_BYTES));
        Ok(())
    }

    pub fn advance_time(
        &mut self,
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
    ) -> Result<(), CongestionError> {
        self.delay.advance_time(
            now_micros,
            smoothed_rtt_micros.unwrap_or(INITIAL_RTO_MICROS).max(1),
        )
    }

    pub fn on_ack(
        &mut self,
        now_micros: u64,
        acknowledged_bytes: usize,
        flight_size_before_ack: usize,
        one_way_delay_micros: u32,
        smoothed_rtt_micros: Option<u64>,
        congestion_limited: bool,
    ) -> Result<CongestionAckOutcome, CongestionError> {
        let horizon = smoothed_rtt_micros.unwrap_or(INITIAL_RTO_MICROS).max(1);
        self.delay
            .add_sample(now_micros, one_way_delay_micros, horizon)?;
        let delay_snapshot = self.delay.snapshot(smoothed_rtt_micros);
        let queue_delay_micros = delay_snapshot.queue_delay_micros.unwrap_or(0);
        let previous_window_bytes = self.congestion_window_bytes();
        if self.slow_start_active
            && self
                .slow_start_exit_delay_micros
                .is_some_and(|exit_delay| queue_delay_micros >= exit_delay)
        {
            let threshold_bytes = previous_window_bytes / 2;
            self.exit_slow_start(Some(threshold_bytes));
            if let Some(percent) = self.slow_start_exit_window_percent {
                let exit_window_bytes =
                    previous_window_bytes.saturating_mul(usize::from(percent)) / 100;
                self.congestion_window_fixed = bytes_to_fixed(
                    exit_window_bytes.max(
                        self.maximum_segment_bytes
                            .saturating_mul(MIN_CONGESTION_PACKETS),
                    ),
                );
            }
        }
        let off_target = i128::from(TARGET_DELAY_MICROS) - i128::from(queue_delay_micros);
        let mut gain_fixed = off_target
            .saturating_mul(acknowledged_bytes as i128)
            .saturating_mul(self.maximum_segment_bytes as i128)
            .saturating_mul(FIXED_POINT_ONE)
            / i128::from(TARGET_DELAY_MICROS)
            / (previous_window_bytes.max(1) as i128);
        let mut used_slow_start = false;
        if self.slow_start_active && congestion_limited {
            let exponential_gain = bytes_to_fixed(acknowledged_bytes);
            if self.slow_start_threshold_bytes.is_some_and(|threshold| {
                previous_window_bytes.saturating_add(acknowledged_bytes) > threshold
            }) {
                self.exit_slow_start(self.slow_start_threshold_bytes);
            } else {
                gain_fixed = gain_fixed.max(exponential_gain);
                used_slow_start = true;
                self.slow_start_acknowledgements =
                    self.slow_start_acknowledgements.saturating_add(1);
            }
        }
        if gain_fixed > 0 && !congestion_limited {
            gain_fixed = 0;
        }
        let mut candidate = self.congestion_window_fixed.saturating_add(gain_fixed);
        if gain_fixed > 0 && !used_slow_start {
            let maximum_allowed =
                bytes_to_fixed(flight_size_before_ack.saturating_add(self.maximum_segment_bytes));
            candidate = candidate.min(maximum_allowed.max(self.congestion_window_fixed));
        }
        self.timeout_collapsed = false;
        let ordinary_floor = bytes_to_fixed(
            self.maximum_segment_bytes
                .saturating_mul(MIN_CONGESTION_PACKETS),
        );
        self.congestion_window_fixed = candidate
            .max(ordinary_floor)
            .min(bytes_to_fixed(MAX_CONGESTION_WINDOW_BYTES));
        let congestion_window_bytes = self.congestion_window_bytes();
        Ok(CongestionAckOutcome {
            previous_window_bytes,
            congestion_window_bytes,
            queue_delay_micros,
            window_delta_bytes: signed_delta(congestion_window_bytes, previous_window_bytes),
        })
    }

    pub fn on_loss(
        &mut self,
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
        isolated_mtu_probe: bool,
    ) -> Result<bool, CongestionError> {
        self.advance_time(now_micros, smoothed_rtt_micros)?;
        if isolated_mtu_probe || now_micros < self.next_loss_reduction_micros {
            self.ignored_loss_events = self.ignored_loss_events.saturating_add(1);
            return Ok(false);
        }
        let minimum = bytes_to_fixed(
            self.maximum_segment_bytes
                .saturating_mul(MIN_CONGESTION_PACKETS),
        );
        self.congestion_window_fixed = (self.congestion_window_fixed / 2).max(minimum);
        self.next_loss_reduction_micros =
            now_micros.saturating_add(smoothed_rtt_micros.unwrap_or(INITIAL_RTO_MICROS).max(1));
        self.loss_reductions = self.loss_reductions.saturating_add(1);
        self.timeout_collapsed = false;
        if self.slow_start_active {
            self.exit_slow_start(Some(self.congestion_window_bytes()));
        }
        Ok(true)
    }

    pub fn on_timeout(
        &mut self,
        now_micros: u64,
        smoothed_rtt_micros: Option<u64>,
    ) -> Result<(), CongestionError> {
        self.advance_time(now_micros, smoothed_rtt_micros)?;
        self.congestion_window_fixed = bytes_to_fixed(self.maximum_segment_bytes);
        self.next_loss_reduction_micros =
            now_micros.saturating_add(smoothed_rtt_micros.unwrap_or(INITIAL_RTO_MICROS).max(1));
        self.timeout_collapses = self.timeout_collapses.saturating_add(1);
        self.timeout_collapsed = true;
        if self.slow_start_exit_delay_micros.is_some() {
            self.slow_start_active = true;
        }
        Ok(())
    }

    fn exit_slow_start(&mut self, threshold_bytes: Option<usize>) {
        if !self.slow_start_active {
            return;
        }
        self.slow_start_active = false;
        self.slow_start_threshold_bytes = threshold_bytes;
        self.slow_start_exits = self.slow_start_exits.saturating_add(1);
    }

    fn congestion_window_bytes(&self) -> usize {
        fixed_to_bytes(self.congestion_window_fixed)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PacerSnapshot {
    pub next_send_micros: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Pacer {
    next_send_micros: u64,
}

impl Pacer {
    #[must_use]
    pub fn snapshot(&self) -> PacerSnapshot {
        PacerSnapshot {
            next_send_micros: self.next_send_micros,
        }
    }

    #[must_use]
    pub fn is_ready(&self, now_micros: u64) -> bool {
        now_micros >= self.next_send_micros
    }

    pub fn on_payload_emitted(
        &mut self,
        now_micros: u64,
        payload_bytes: usize,
        congestion_window_bytes: usize,
        smoothed_rtt_micros: Option<u64>,
    ) -> u64 {
        let interval = smoothed_rtt_micros.map_or(0, |rtt| {
            (payload_bytes as u128)
                .saturating_mul(u128::from(rtt))
                .div_ceil(congestion_window_bytes.max(1) as u128)
                .min(u128::from(u64::MAX)) as u64
        });
        let base = self.next_send_micros.max(now_micros);
        self.next_send_micros = base.saturating_add(interval);
        self.next_send_micros
    }

    pub fn reset(&mut self, now_micros: u64) {
        self.next_send_micros = now_micros;
    }
}

fn validate_mss(maximum_segment_bytes: usize) -> Result<(), CongestionError> {
    if maximum_segment_bytes == 0 || maximum_segment_bytes > MAX_UTP_PAYLOAD_SIZE {
        return Err(CongestionError::InvalidMaximumSegmentSize {
            bytes: maximum_segment_bytes,
            maximum: MAX_UTP_PAYLOAD_SIZE,
        });
    }
    Ok(())
}

fn bytes_to_fixed(bytes: usize) -> i128 {
    (bytes as i128).saturating_mul(FIXED_POINT_ONE)
}

fn fixed_to_bytes(fixed: i128) -> usize {
    let bytes = fixed.max(0) >> FIXED_POINT_SHIFT;
    bytes.min(MAX_CONGESTION_WINDOW_BYTES as i128) as usize
}

fn signed_delta(current: usize, previous: usize) -> i64 {
    if current >= previous {
        current.saturating_sub(previous).min(i64::MAX as usize) as i64
    } else {
        -(previous.saturating_sub(current).min(i64::MAX as usize) as i64)
    }
}

fn wrapping_delay_less(left: u32, right: u32) -> bool {
    right.wrapping_sub(left) < left.wrapping_sub(right)
}

fn wrapping_min(values: impl IntoIterator<Item = u32>) -> Option<u32> {
    values.into_iter().fold(None, |minimum, value| {
        Some(match minimum {
            Some(current) if !wrapping_delay_less(value, current) => current,
            Some(_) | None => value,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_controller(maximum_segment_bytes: usize) -> CongestionController {
        CongestionController::with_startup(maximum_segment_bytes, CongestionStartup::LinearLedbat)
            .expect("linear controller")
    }

    #[test]
    fn delay_history_rolls_ten_minute_buckets_and_expires_idle_state() {
        let mut history = DelayHistory::new();
        let first = history
            .add_sample(0, 1_000, 1_000_000)
            .expect("first sample");
        assert_eq!(first.base_delay_micros, Some(1_000));
        history
            .add_sample(MICROS_PER_MINUTE, 1_100, 1_000_000)
            .expect("second bucket");
        assert_eq!(history.snapshot(None).queue_delay_micros, Some(100));

        history
            .add_sample(MICROS_PER_MINUTE * 10, 1_200, 1_000_000)
            .expect("expire first bucket");
        assert_eq!(history.snapshot(None).base_delay_micros, Some(1_100));
        history
            .advance_time(MICROS_PER_MINUTE * 20, 1_000_000)
            .expect("idle expiry");
        assert_eq!(history.snapshot(None).base_delay_micros, None);
        assert_eq!(history.snapshot(None).current_samples, 0);
    }

    #[test]
    fn delay_samples_handle_wire_wrap_offset_and_rtt_clamping() {
        let mut history = DelayHistory::new();
        history
            .add_sample(0, u32::MAX - 10, 1_000_000)
            .expect("base near wrap");
        history.advance_time(2, 1).expect("expire base sample");
        history.add_sample(2, 5, 1).expect("wrapped sample");
        assert_eq!(history.snapshot(None).queue_delay_micros, Some(16));
        history.add_sample(2, 25, 1).expect("queue sample");
        assert_eq!(history.snapshot(None).queue_delay_micros, Some(16));

        history.advance_time(1_000_003, 1).expect("prune old");
        history
            .add_sample(1_000_003, 1_000_025, 1)
            .expect("large queue sample");
        assert_eq!(
            history.snapshot(Some(50_000)).queue_delay_micros,
            Some(50_000)
        );
    }

    #[test]
    fn ledbat_growth_target_and_decrease_follow_fixed_point_formula() {
        let mut controller = linear_controller(1_000);
        let growth = controller
            .on_ack(0, 1_000, 2_000, 10_000, Some(500_000), true)
            .expect("growth ACK");
        assert_eq!(growth.queue_delay_micros, 0);
        assert_eq!(growth.congestion_window_bytes, 2_500);
        assert_eq!(growth.window_delta_bytes, 500);

        let target = controller
            .on_ack(500_001, 1_000, 2_500, 110_000, Some(500_000), true)
            .expect("target ACK");
        assert_eq!(target.queue_delay_micros, 100_000);
        assert_eq!(target.congestion_window_bytes, 2_500);

        let decrease = controller
            .on_ack(1_000_002, 1_000, 2_500, 210_000, Some(500_000), true)
            .expect("high-delay ACK");
        assert_eq!(decrease.queue_delay_micros, 200_000);
        assert_eq!(decrease.congestion_window_bytes, 2_100);
        assert_eq!(decrease.window_delta_bytes, -400);
    }

    #[test]
    fn persistent_queue_delay_decreases_but_never_below_two_mss() {
        let mut controller = linear_controller(1_000);
        controller
            .on_ack(0, 1_000, 2_000, 10_000, Some(500_000), true)
            .expect("establish base");
        let result = controller
            .on_ack(1, 1_000, 2_500, 210_000, Some(500_000), true)
            .expect("high queue");
        assert_eq!(result.queue_delay_micros, 0);

        controller
            .advance_time(500_002, Some(1))
            .expect("expire current minimum");
        let result = controller
            .on_ack(500_002, 1_000, 2_900, 210_000, Some(500_000), true)
            .expect("persistent high queue");
        assert_eq!(result.queue_delay_micros, 200_000);
        assert!(result.window_delta_bytes < 0);
        assert!(result.congestion_window_bytes >= 2_000);
    }

    #[test]
    fn application_limited_positive_gain_is_suppressed() {
        let mut controller = CongestionController::new(1_000).expect("controller");
        let result = controller
            .on_ack(0, 100, 100, 10_000, Some(20_000), false)
            .expect("application-limited ACK");
        assert_eq!(result.congestion_window_bytes, 2_000);
        assert_eq!(result.window_delta_bytes, 0);
        assert!(controller.snapshot().slow_start_active);
        assert_eq!(controller.snapshot().slow_start_acknowledgements, 0);
    }

    #[test]
    fn bounded_slow_start_grows_by_acked_bytes_and_exits_at_ten_milliseconds() {
        let mut controller = CongestionController::new(1_000).expect("controller");
        let growth = controller
            .on_ack(0, 2_000, 2_000, 10_000, Some(500_000), true)
            .expect("slow-start ACK");
        assert_eq!(growth.congestion_window_bytes, 4_000);
        assert!(controller.snapshot().slow_start_active);
        assert_eq!(controller.snapshot().slow_start_acknowledgements, 1);

        let exit = controller
            .on_ack(500_001, 1_000, 4_000, 20_000, Some(500_000), true)
            .expect("startup exit ACK");
        assert_eq!(exit.queue_delay_micros, STARTUP_EXIT_DELAY_MICROS);
        assert_eq!(exit.previous_window_bytes, 4_000);
        assert_eq!(exit.congestion_window_bytes, 2_225);
        assert!(!controller.snapshot().slow_start_active);
        assert_eq!(
            controller.snapshot().slow_start_threshold_bytes,
            Some(2_000)
        );
        assert_eq!(controller.snapshot().slow_start_exits, 1);
    }

    #[test]
    fn bounded_slow_start_loss_threshold_bounds_timeout_restart() {
        let mut controller = CongestionController::new(1_000).expect("controller");
        controller
            .on_ack(0, 1_000, 2_000, 10_000, Some(20_000), true)
            .expect("initial slow-start ACK");
        assert!(
            controller
                .on_loss(10_000, Some(20_000), false)
                .expect("loss reduction")
        );
        assert!(!controller.snapshot().slow_start_active);
        assert_eq!(
            controller.snapshot().slow_start_threshold_bytes,
            Some(2_000)
        );

        controller
            .on_timeout(30_001, Some(20_000))
            .expect("timeout restart");
        assert!(controller.snapshot().slow_start_active);
        let threshold = controller
            .on_ack(30_002, 1_000, 1_000, 10_000, Some(20_000), true)
            .expect("threshold ACK");
        assert_eq!(threshold.congestion_window_bytes, 2_000);
        assert!(controller.snapshot().slow_start_active);
        controller
            .on_ack(30_003, 1_000, 2_000, 10_000, Some(20_000), true)
            .expect("threshold exit");
        assert!(!controller.snapshot().slow_start_active);
        assert_eq!(controller.snapshot().slow_start_exits, 2);
    }

    #[test]
    fn isolated_mtu_probe_loss_keeps_bounded_startup_active() {
        let mut controller = CongestionController::new(1_000).expect("controller");
        controller
            .on_ack(0, 1_000, 2_000, 10_000, Some(20_000), true)
            .expect("slow-start ACK");
        assert!(
            !controller
                .on_loss(10_000, Some(20_000), true)
                .expect("isolated probe")
        );
        let snapshot = controller.snapshot();
        assert!(snapshot.slow_start_active);
        assert_eq!(snapshot.slow_start_exits, 0);
        assert_eq!(snapshot.ignored_loss_events, 1);
        assert_eq!(snapshot.congestion_window_bytes, 3_000);
    }

    #[test]
    fn loss_reduces_once_per_rtt_and_ignores_isolated_mtu_probe() {
        let mut controller = CongestionController::new(1_000).expect("controller");
        controller
            .on_ack(0, 1_000, 2_000, 10_000, Some(20_000), true)
            .expect("grow");
        assert!(
            controller
                .on_loss(10_000, Some(20_000), false)
                .expect("first loss")
        );
        assert!(
            !controller
                .on_loss(20_000, Some(20_000), false)
                .expect("same epoch loss")
        );
        assert!(
            !controller
                .on_loss(30_000, Some(20_000), true)
                .expect("isolated probe")
        );
        assert!(
            controller
                .on_loss(30_000, Some(20_000), false)
                .expect("next epoch loss")
        );
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.loss_reductions, 2);
        assert_eq!(snapshot.ignored_loss_events, 2);
        assert_eq!(snapshot.congestion_window_bytes, 2_000);
    }

    #[test]
    fn timeout_collapses_to_one_mss_and_ack_restores_ordinary_floor() {
        let mut controller = CongestionController::new(1_000).expect("controller");
        controller
            .on_timeout(10, Some(20_000))
            .expect("timeout collapse");
        assert_eq!(controller.snapshot().congestion_window_bytes, 1_000);
        assert!(controller.snapshot().timeout_collapsed);
        controller
            .on_ack(20, 1, 1_000, 10_000, Some(20_000), false)
            .expect("recovery ACK");
        assert_eq!(controller.snapshot().congestion_window_bytes, 2_000);
        assert!(!controller.snapshot().timeout_collapsed);
    }

    #[test]
    fn time_reversal_and_invalid_mss_are_atomic() {
        assert!(matches!(
            CongestionController::new(0),
            Err(CongestionError::InvalidMaximumSegmentSize { .. })
        ));
        let mut controller = CongestionController::new(1_000).expect("controller");
        controller.advance_time(100, Some(20_000)).expect("advance");
        let before = controller.snapshot();
        assert!(matches!(
            controller.on_ack(99, 1_000, 2_000, 10_000, Some(20_000), true),
            Err(CongestionError::TimeReversed { .. })
        ));
        assert_eq!(controller.snapshot(), before);
    }

    #[test]
    fn pacing_uses_payload_rtt_and_window_with_an_idle_reset() {
        let mut pacer = Pacer::default();
        assert!(pacer.is_ready(100));
        assert_eq!(
            pacer.on_payload_emitted(100, 500, 2_000, Some(20_000)),
            5_100
        );
        assert!(!pacer.is_ready(5_099));
        assert_eq!(
            pacer.on_payload_emitted(5_100, 1_000, 2_000, Some(20_000)),
            15_100
        );
        pacer.reset(20_000);
        assert!(pacer.is_ready(20_000));
        assert_eq!(pacer.on_payload_emitted(20_000, 500, 2_000, None), 20_000);
    }
}

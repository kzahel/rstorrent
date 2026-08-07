use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};

use rstorrent_protocol::storage_layout::RequiredPayloadGeometry;
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{TorrentEtaView, TorrentView, ViewHub};

pub(super) const ETA_TICK_INTERVAL: Duration = Duration::from_secs(1);
const ETA_STALL_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TorrentEtaModel {
    geometry: Option<RequiredPayloadGeometry>,
    remaining_payload_bytes: Option<u64>,
    generation: u64,
    reserved_generation: Option<u64>,
    active_generation: Option<u64>,
    transfer_applicable: bool,
    activated_at: Option<Instant>,
    last_tick: Option<Instant>,
    last_accepted: Option<Instant>,
    accepted_since_tick: u64,
    smoothed_rate: u64,
    has_usable_sample: bool,
    public_rate: u64,
    eta: TorrentEtaView,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TorrentEtaError {
    GenerationExhausted,
    GenerationAlreadyOwned,
    ArithmeticOverflow,
    RemainingUnderflow,
    FailedBytesExceedRequired,
}

impl fmt::Display for TorrentEtaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::GenerationExhausted => "torrent ETA generation exhausted",
            Self::GenerationAlreadyOwned => "torrent ETA generation is already owned",
            Self::ArithmeticOverflow => "torrent ETA arithmetic overflow",
            Self::RemainingUnderflow => "torrent ETA remaining payload underflow",
            Self::FailedBytesExceedRequired => {
                "torrent ETA failed payload exceeds required payload"
            }
        })
    }
}

impl Error for TorrentEtaError {}

impl Default for TorrentEtaModel {
    fn default() -> Self {
        Self {
            geometry: None,
            remaining_payload_bytes: None,
            generation: 0,
            reserved_generation: None,
            active_generation: None,
            transfer_applicable: false,
            activated_at: None,
            last_tick: None,
            last_accepted: None,
            accepted_since_tick: 0,
            smoothed_rate: 0,
            has_usable_sample: false,
            public_rate: 0,
            eta: TorrentEtaView::Unavailable,
        }
    }
}

impl TorrentEtaModel {
    pub(super) fn reserve_generation(&mut self) -> Result<u64, TorrentEtaError> {
        if self.active_generation.is_some() {
            return Err(TorrentEtaError::GenerationAlreadyOwned);
        }
        self.reserved_generation = None;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(TorrentEtaError::GenerationExhausted)?;
        self.reserved_generation = Some(self.generation);
        self.reset_rate();
        Ok(self.generation)
    }

    pub(super) fn activate_generation(&mut self, generation: u64, now: Instant) -> bool {
        if self.reserved_generation != Some(generation) {
            return false;
        }
        self.reserved_generation = None;
        self.active_generation = Some(generation);
        self.start_rate(now);
        true
    }

    pub(super) fn deactivate_generation(&mut self, generation: u64) -> bool {
        let matches = self.reserved_generation == Some(generation)
            || self.active_generation == Some(generation);
        if !matches {
            return false;
        }
        self.reserved_generation = None;
        self.active_generation = None;
        self.reconstruct_remaining();
        self.reset_rate();
        true
    }

    pub(super) fn reconcile_geometry(
        &mut self,
        geometry: Option<RequiredPayloadGeometry>,
        selection_unchanged: bool,
        transfer_applicable: bool,
        now: Instant,
    ) {
        let previous_required = self
            .geometry
            .map(|geometry| geometry.required_payload_bytes);
        let next_required = geometry.map(|geometry| geometry.required_payload_bytes);
        let preserve_generation = selection_unchanged
            && (previous_required.is_none() || previous_required == next_required);

        self.geometry = geometry;
        if !preserve_generation {
            self.reserved_generation = None;
            self.active_generation = None;
            self.reconstruct_remaining();
            self.reset_rate();
        } else if (self.remaining_payload_bytes.is_none() && self.geometry.is_some())
            || (self.active_generation.is_none() && self.reserved_generation.is_none())
        {
            self.reconstruct_remaining();
        }

        self.set_transfer_applicable(transfer_applicable, now);
        self.refresh_eta(now);
    }

    pub(super) fn set_transfer_applicable(&mut self, applicable: bool, now: Instant) {
        let was_applicable = self.transfer_applicable;
        self.transfer_applicable = applicable;
        if !applicable {
            self.reset_rate();
        } else if !was_applicable && self.active_generation.is_some() {
            self.start_rate(now);
        }
        self.refresh_eta(now);
    }

    pub(super) fn block_received(
        &mut self,
        generation: u64,
        length: u32,
        now: Instant,
    ) -> Result<bool, TorrentEtaError> {
        if self.active_generation != Some(generation) || !self.transfer_applicable {
            return Ok(false);
        }
        let Some(remaining) = self.remaining_payload_bytes.as_mut() else {
            return Ok(false);
        };
        *remaining = remaining
            .checked_sub(u64::from(length))
            .ok_or(TorrentEtaError::RemainingUnderflow)?;
        self.accepted_since_tick = self
            .accepted_since_tick
            .checked_add(u64::from(length))
            .ok_or(TorrentEtaError::ArithmeticOverflow)?;
        self.last_accepted = Some(now);
        if *remaining == 0 {
            self.refresh_eta(now);
        }
        Ok(true)
    }

    pub(super) fn piece_hash_failed(
        &mut self,
        generation: u64,
        failed_bytes: usize,
        now: Instant,
    ) -> Result<bool, TorrentEtaError> {
        if self.active_generation != Some(generation) || !self.transfer_applicable {
            return Ok(false);
        }
        let failed_bytes =
            u64::try_from(failed_bytes).map_err(|_| TorrentEtaError::ArithmeticOverflow)?;
        let Some(geometry) = self.geometry else {
            return Ok(false);
        };
        let Some(remaining) = self.remaining_payload_bytes.as_mut() else {
            return Ok(false);
        };
        let restored = remaining
            .checked_add(failed_bytes)
            .ok_or(TorrentEtaError::ArithmeticOverflow)?;
        if restored > geometry.required_payload_bytes {
            return Err(TorrentEtaError::FailedBytesExceedRequired);
        }
        *remaining = restored;
        self.refresh_eta(now);
        Ok(true)
    }

    pub(super) fn tick(&mut self, now: Instant) -> Result<bool, TorrentEtaError> {
        if self.active_generation.is_none() || !self.transfer_applicable {
            return Ok(false);
        }
        let Some(previous_tick) = self.last_tick else {
            self.last_tick = Some(now);
            self.refresh_eta(now);
            return Ok(true);
        };
        let elapsed = now.saturating_duration_since(previous_tick);
        let elapsed_millis = elapsed.as_millis();
        if elapsed_millis == 0 {
            return Ok(false);
        }
        let sample = u128::from(self.accepted_since_tick)
            .checked_mul(1_000)
            .ok_or(TorrentEtaError::ArithmeticOverflow)?
            / elapsed_millis;
        let sample = u64::try_from(sample).map_err(|_| TorrentEtaError::ArithmeticOverflow)?;
        self.accepted_since_tick = 0;
        self.last_tick = Some(now);
        if !self.has_usable_sample && sample > 0 {
            self.smoothed_rate = sample;
            self.has_usable_sample = true;
        } else if self.has_usable_sample {
            let smoothed = u128::from(self.smoothed_rate) * 4 / 5 + u128::from(sample) / 5;
            self.smoothed_rate =
                u64::try_from(smoothed).map_err(|_| TorrentEtaError::ArithmeticOverflow)?;
        }
        self.refresh_eta(now);
        Ok(true)
    }

    pub(super) fn apply_to_view(&self, view: &mut TorrentView) {
        view.required_payload_bytes = self
            .geometry
            .map(|geometry| geometry.required_payload_bytes.to_string());
        view.remaining_payload_bytes = self.remaining_payload_bytes.map(|bytes| bytes.to_string());
        view.eta_payload_download_rate_bytes = self.public_rate.to_string();
        view.eta = self.eta.clone();
    }

    pub(super) fn fail_closed(&mut self) {
        self.reserved_generation = None;
        self.active_generation = None;
        self.reset_rate();
    }

    fn reconstruct_remaining(&mut self) {
        self.remaining_payload_bytes = self.geometry.and_then(|geometry| {
            geometry
                .required_payload_bytes
                .checked_sub(geometry.verified_required_payload_bytes)
        });
    }

    fn start_rate(&mut self, now: Instant) {
        self.reset_rate();
        self.activated_at = Some(now);
        self.last_tick = Some(now);
        self.refresh_eta(now);
    }

    fn reset_rate(&mut self) {
        self.activated_at = None;
        self.last_tick = None;
        self.last_accepted = None;
        self.accepted_since_tick = 0;
        self.smoothed_rate = 0;
        self.has_usable_sample = false;
        self.public_rate = 0;
        self.eta = TorrentEtaView::Unavailable;
    }

    fn refresh_eta(&mut self, now: Instant) {
        let Some(remaining) = self.remaining_payload_bytes else {
            self.public_rate = 0;
            self.eta = TorrentEtaView::Unavailable;
            return;
        };
        if self.active_generation.is_none()
            || !self.transfer_applicable
            || remaining == 0
            || self
                .geometry
                .is_some_and(|geometry| geometry.required_payload_bytes == 0)
        {
            self.public_rate = 0;
            self.eta = TorrentEtaView::Unavailable;
            return;
        }
        let progress_origin = self.last_accepted.or(self.activated_at);
        if progress_origin
            .is_some_and(|origin| now.saturating_duration_since(origin) >= ETA_STALL_INTERVAL)
        {
            self.public_rate = 0;
            self.eta = TorrentEtaView::Stalled;
            return;
        }
        if self.has_usable_sample && self.smoothed_rate > 0 {
            self.public_rate = self.smoothed_rate;
            let seconds =
                remaining / self.smoothed_rate + u64::from(remaining % self.smoothed_rate != 0);
            self.eta = TorrentEtaView::Estimate {
                seconds: seconds.to_string(),
            };
        } else {
            self.public_rate = 0;
            self.eta = TorrentEtaView::WarmingUp;
        }
    }

    #[cfg(test)]
    fn remaining_payload_bytes(&self) -> Option<u64> {
        self.remaining_payload_bytes
    }
}

#[derive(Debug)]
pub(crate) struct TorrentEtaRuntime {
    cancellation: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl TorrentEtaRuntime {
    pub(crate) fn start(views: ViewHub) -> Self {
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut timer = tokio::time::interval(ETA_TICK_INTERVAL);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            timer.tick().await;
            loop {
                tokio::select! {
                    biased;
                    _ = task_cancellation.cancelled() => break,
                    _ = timer.tick() => {
                        let _ = views.record_eta_tick(Instant::now());
                    }
                }
            }
        });
        Self {
            cancellation,
            task: Some(task),
        }
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), JoinError> {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.await?;
        }
        Ok(())
    }
}

impl Drop for TorrentEtaRuntime {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(required: u64, verified: u64) -> RequiredPayloadGeometry {
        RequiredPayloadGeometry {
            required_payload_bytes: required,
            verified_required_payload_bytes: verified,
        }
    }

    fn active_model(now: Instant) -> (TorrentEtaModel, u64) {
        let mut model = TorrentEtaModel::default();
        model.reconcile_geometry(Some(geometry(10_000, 2_000)), true, true, now);
        let generation = model.reserve_generation().expect("reserve generation");
        assert!(model.activate_generation(generation, now));
        (model, generation)
    }

    #[test]
    fn warms_estimates_decays_stalls_and_recovers() {
        let now = Instant::now();
        let (mut model, generation) = active_model(now);
        assert_eq!(model.eta, TorrentEtaView::WarmingUp);
        assert!(
            model
                .block_received(generation, 1_000, now + Duration::from_millis(500))
                .expect("accept block")
        );
        model
            .tick(now + Duration::from_secs(1))
            .expect("first tick");
        assert_eq!(model.public_rate, 1_000);
        assert_eq!(
            model.eta,
            TorrentEtaView::Estimate {
                seconds: "7".to_owned(),
            }
        );

        for second in 2..10 {
            model
                .tick(now + Duration::from_secs(second))
                .expect("idle tick");
        }
        assert!(matches!(model.eta, TorrentEtaView::Estimate { .. }));
        model
            .tick(now + Duration::from_secs(11))
            .expect("stall tick");
        assert_eq!(model.eta, TorrentEtaView::Stalled);
        assert_eq!(model.public_rate, 0);

        model
            .block_received(generation, 500, now + Duration::from_millis(11_500))
            .expect("resume block");
        model
            .tick(now + Duration::from_secs(12))
            .expect("resume tick");
        assert!(matches!(model.eta, TorrentEtaView::Estimate { .. }));
    }

    #[test]
    fn hash_failure_restores_work_and_stale_generation_is_ignored() {
        let now = Instant::now();
        let (mut model, generation) = active_model(now);
        model
            .block_received(generation, 1_000, now)
            .expect("receive block");
        assert_eq!(model.remaining_payload_bytes(), Some(7_000));
        model
            .piece_hash_failed(generation, 1_000, now)
            .expect("restore failed bytes");
        assert_eq!(model.remaining_payload_bytes(), Some(8_000));

        assert!(model.deactivate_generation(generation));
        assert!(
            !model
                .block_received(generation, 1_000, now)
                .expect("ignore stale block")
        );
        assert_eq!(model.remaining_payload_bytes(), Some(8_000));
    }

    #[test]
    fn selection_replacement_fences_generation_and_reconstructs() {
        let now = Instant::now();
        let (mut model, generation) = active_model(now);
        model
            .block_received(generation, 1_000, now)
            .expect("receive block");
        model.reconcile_geometry(Some(geometry(20_000, 4_000)), false, true, now);
        assert_eq!(model.remaining_payload_bytes(), Some(16_000));
        assert_eq!(model.eta, TorrentEtaView::Unavailable);
        assert!(
            !model
                .block_received(generation, 1_000, now)
                .expect("ignore old selection generation")
        );
    }

    #[test]
    fn all_skipped_and_complete_are_unavailable() {
        let now = Instant::now();
        let mut model = TorrentEtaModel::default();
        model.reconcile_geometry(Some(geometry(0, 0)), true, true, now);
        let generation = model.reserve_generation().expect("reserve generation");
        assert!(model.activate_generation(generation, now));
        assert_eq!(model.eta, TorrentEtaView::Unavailable);

        model.reconcile_geometry(Some(geometry(10_000, 10_000)), false, false, now);
        assert_eq!(model.remaining_payload_bytes(), Some(0));
        assert_eq!(model.eta, TorrentEtaView::Unavailable);
    }

    #[test]
    fn no_payload_stays_warming_for_nine_seconds_then_stalls() {
        let now = Instant::now();
        let (mut model, _) = active_model(now);
        model
            .tick(now + Duration::from_secs(9))
            .expect("nine-second tick");
        assert_eq!(model.eta, TorrentEtaView::WarmingUp);
        model
            .tick(now + Duration::from_secs(10))
            .expect("ten-second tick");
        assert_eq!(model.eta, TorrentEtaView::Stalled);
        assert_eq!(model.public_rate, 0);
    }

    #[test]
    fn irregular_ticks_use_elapsed_time_and_zero_elapsed_keeps_the_bucket() {
        let now = Instant::now();
        let (mut model, generation) = active_model(now);
        model
            .block_received(generation, 3_000, now + Duration::from_secs(2))
            .expect("late accepted bytes");
        model.tick(now + Duration::from_secs(3)).expect("late tick");
        assert_eq!(model.smoothed_rate, 1_000);

        model
            .block_received(generation, 1_000, now + Duration::from_secs(3))
            .expect("same-instant accepted bytes");
        assert!(
            !model
                .tick(now + Duration::from_secs(3))
                .expect("zero elapsed tick")
        );
        model
            .block_received(generation, 1_000, now + Duration::from_secs(4))
            .expect("next accepted bytes");
        model
            .tick(now + Duration::from_secs(5))
            .expect("two-second tick");
        assert_eq!(model.smoothed_rate, 1_000);
    }

    #[test]
    fn maximum_scalars_keep_ceiling_eta_exact_without_overflow() {
        let now = Instant::now();
        let mut model = TorrentEtaModel::default();
        model.reconcile_geometry(Some(geometry(u64::MAX, 0)), true, true, now);
        let generation = model.reserve_generation().expect("reserve generation");
        assert!(model.activate_generation(generation, now));
        model
            .block_received(generation, u32::MAX, now)
            .expect("maximum block scalar");
        model
            .tick(now + Duration::from_millis(1))
            .expect("maximum sample tick");

        let rate = u64::from(u32::MAX) * 1_000;
        let remaining = u64::MAX - u64::from(u32::MAX);
        let seconds = remaining / rate + u64::from(remaining % rate != 0);
        assert_eq!(model.public_rate, rate);
        assert_eq!(
            model.eta,
            TorrentEtaView::Estimate {
                seconds: seconds.to_string(),
            }
        );
    }

    #[test]
    fn retained_eta_model_has_constant_scalar_size() {
        let retained = std::mem::size_of::<TorrentEtaModel>();
        eprintln!("retained torrent ETA model bytes={retained}");
        assert!(retained <= 256);
        assert_eq!(std::mem::size_of::<RequiredPayloadGeometry>(), 16);
    }

    #[test]
    fn tagged_eta_contract_round_trips_every_state() {
        let states = [
            TorrentEtaView::Estimate {
                seconds: "252".to_owned(),
            },
            TorrentEtaView::WarmingUp,
            TorrentEtaView::Stalled,
            TorrentEtaView::Unavailable,
        ];
        for state in states {
            let encoded = serde_json::to_string(&state).expect("serialize ETA state");
            let decoded: TorrentEtaView =
                serde_json::from_str(&encoded).expect("deserialize ETA state");
            assert_eq!(decoded, state);
        }
    }
}

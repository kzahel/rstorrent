//! Runtime-independent combined automatic download and seed admission policy.

use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TorrentAdmissionKind {
    Download { queue_position: Option<i64> },
    Seed { rank: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TorrentAdmissionState {
    pub(crate) torrent_id: String,
    pub(crate) desired_running: bool,
    pub(crate) checking: bool,
    pub(crate) blocked: bool,
    pub(crate) kind: TorrentAdmissionKind,
    pub(crate) active_generation: Option<u64>,
    /// Only an already-active torrent may be exempt from its type limit.
    pub(crate) inactive: bool,
}

impl TorrentAdmissionState {
    fn eligible(&self) -> bool {
        self.desired_running
            && !self.checking
            && !self.blocked
            && match self.kind {
                TorrentAdmissionKind::Download { queue_position } => queue_position.is_some(),
                TorrentAdmissionKind::Seed { .. } => true,
            }
    }

    fn download_order(&self) -> (i64, &str) {
        let TorrentAdmissionKind::Download { queue_position } = self.kind else {
            unreachable!("download order is used only for downloads");
        };
        (queue_position.unwrap_or(i64::MAX), self.torrent_id.as_str())
    }

    fn admission_type(&self) -> TorrentAdmissionType {
        match self.kind {
            TorrentAdmissionKind::Download { .. } => TorrentAdmissionType::Download,
            TorrentAdmissionKind::Seed { .. } => TorrentAdmissionType::Seed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TorrentAdmissionType {
    Download,
    Seed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionStopReason {
    Ineligible,
    Capacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionAction {
    Start,
    Retain {
        generation: u64,
    },
    Stop {
        generation: u64,
        reason: AdmissionStopReason,
    },
    Idle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TorrentAdmissionDecision {
    pub(crate) torrent_id: String,
    pub(crate) admission_type: TorrentAdmissionType,
    pub(crate) action: AdmissionAction,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TorrentAutoManager;

impl TorrentAutoManager {
    pub(crate) fn reconcile(
        torrents: &[TorrentAdmissionState],
        download_limit: usize,
        seed_limit: usize,
        hard_limit: usize,
    ) -> Vec<TorrentAdmissionDecision> {
        let mut winners = BTreeSet::new();
        let mut hard_remaining = hard_limit;
        let mut download_slots = download_limit;

        // Preserve healthy active downloads before considering queued work.
        // Inactive active downloads consume the hard ceiling but not a
        // download-type slot.
        let mut active_downloads = torrents
            .iter()
            .filter(|torrent| {
                torrent.eligible()
                    && matches!(torrent.kind, TorrentAdmissionKind::Download { .. })
                    && torrent.active_generation.is_some()
            })
            .collect::<Vec<_>>();
        active_downloads.sort_unstable_by_key(|torrent| torrent.download_order());
        for torrent in active_downloads {
            if hard_remaining == 0 {
                break;
            }
            if torrent.inactive || download_slots != 0 {
                winners.insert(torrent.torrent_id.as_str());
                hard_remaining -= 1;
                if !torrent.inactive {
                    download_slots -= 1;
                }
            }
        }

        let mut queued_downloads = torrents
            .iter()
            .filter(|torrent| {
                torrent.eligible()
                    && matches!(torrent.kind, TorrentAdmissionKind::Download { .. })
                    && torrent.active_generation.is_none()
            })
            .collect::<Vec<_>>();
        queued_downloads.sort_unstable_by_key(|torrent| torrent.download_order());
        for torrent in queued_downloads {
            if hard_remaining == 0 || download_slots == 0 {
                break;
            }
            winners.insert(torrent.torrent_id.as_str());
            hard_remaining -= 1;
            download_slots -= 1;
        }

        // Seeds are always reconsidered in descending exact rank. Canonical
        // identity is the stable final tie-break. Only an already-active slow
        // seed is type-slot exempt; a queued seed must first win a counted
        // slot and observe the continuous inactivity delay.
        let mut seeds = torrents
            .iter()
            .filter(|torrent| {
                torrent.eligible() && matches!(torrent.kind, TorrentAdmissionKind::Seed { .. })
            })
            .collect::<Vec<_>>();
        seeds.sort_unstable_by(|left, right| {
            let TorrentAdmissionKind::Seed { rank: left_rank } = left.kind else {
                unreachable!();
            };
            let TorrentAdmissionKind::Seed { rank: right_rank } = right.kind else {
                unreachable!();
            };
            right_rank
                .cmp(&left_rank)
                .then_with(|| left.torrent_id.cmp(&right.torrent_id))
        });
        let mut seed_slots = seed_limit;
        for torrent in seeds {
            if hard_remaining == 0 {
                break;
            }
            let exempt = torrent.active_generation.is_some() && torrent.inactive;
            if exempt || seed_slots != 0 {
                winners.insert(torrent.torrent_id.as_str());
                hard_remaining -= 1;
                if !exempt {
                    seed_slots -= 1;
                }
            }
        }

        torrents
            .iter()
            .map(|torrent| {
                let wins = winners.contains(torrent.torrent_id.as_str());
                let action = match torrent.active_generation {
                    Some(generation) if wins => AdmissionAction::Retain { generation },
                    Some(generation) => AdmissionAction::Stop {
                        generation,
                        reason: if torrent.eligible() {
                            AdmissionStopReason::Capacity
                        } else {
                            AdmissionStopReason::Ineligible
                        },
                    },
                    None if wins => AdmissionAction::Start,
                    None => AdmissionAction::Idle,
                };
                TorrentAdmissionDecision {
                    torrent_id: torrent.torrent_id.clone(),
                    admission_type: torrent.admission_type(),
                    action,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AdmissionAction, AdmissionStopReason, TorrentAdmissionKind, TorrentAdmissionState,
        TorrentAdmissionType, TorrentAutoManager,
    };

    fn download(id: &str, position: i64) -> TorrentAdmissionState {
        TorrentAdmissionState {
            torrent_id: id.to_owned(),
            desired_running: true,
            checking: false,
            blocked: false,
            kind: TorrentAdmissionKind::Download {
                queue_position: Some(position),
            },
            active_generation: None,
            inactive: false,
        }
    }

    fn seed(id: &str, rank: u32) -> TorrentAdmissionState {
        TorrentAdmissionState {
            torrent_id: id.to_owned(),
            desired_running: true,
            checking: false,
            blocked: false,
            kind: TorrentAdmissionKind::Seed { rank },
            active_generation: None,
            inactive: false,
        }
    }

    fn action(decisions: &[super::TorrentAdmissionDecision], id: &str) -> AdmissionAction {
        decisions
            .iter()
            .find(|decision| decision.torrent_id == id)
            .expect("decision")
            .action
    }

    #[test]
    fn starts_downloads_in_durable_order_up_to_the_limit() {
        let torrents = [
            download("later", 20),
            download("first", 10),
            download("last", 30),
        ];
        let decisions = TorrentAutoManager::reconcile(&torrents, 2, 5, 500);
        assert_eq!(action(&decisions, "first"), AdmissionAction::Start);
        assert_eq!(action(&decisions, "later"), AdmissionAction::Start);
        assert_eq!(action(&decisions, "last"), AdmissionAction::Idle);
    }

    #[test]
    fn healthy_active_downloads_are_not_preempted_by_a_queue_move() {
        let mut active = download("active", 20);
        active.active_generation = Some(7);
        let queued_at_head = download("queued", 10);
        let decisions = TorrentAutoManager::reconcile(&[active, queued_at_head], 1, 5, 500);
        assert_eq!(
            action(&decisions, "active"),
            AdmissionAction::Retain { generation: 7 }
        );
        assert_eq!(action(&decisions, "queued"), AdmissionAction::Idle);
    }

    #[test]
    fn shrinking_demotes_the_latest_active_download_positions() {
        let mut first = download("first", 10);
        first.active_generation = Some(1);
        let mut second = download("second", 20);
        second.active_generation = Some(2);
        let decisions = TorrentAutoManager::reconcile(&[second, first], 1, 5, 500);
        assert_eq!(
            action(&decisions, "first"),
            AdmissionAction::Retain { generation: 1 }
        );
        assert_eq!(
            action(&decisions, "second"),
            AdmissionAction::Stop {
                generation: 2,
                reason: AdmissionStopReason::Capacity,
            }
        );
    }

    #[test]
    fn ineligible_active_torrent_stops_and_opens_capacity() {
        let mut paused = download("paused", 10);
        paused.desired_running = false;
        paused.active_generation = Some(4);
        let next = download("next", 20);
        let decisions = TorrentAutoManager::reconcile(&[paused, next], 1, 5, 500);
        assert_eq!(
            action(&decisions, "paused"),
            AdmissionAction::Stop {
                generation: 4,
                reason: AdmissionStopReason::Ineligible,
            }
        );
        assert_eq!(action(&decisions, "next"), AdmissionAction::Start);
    }

    #[test]
    fn seed_rank_preempts_lower_rank_but_goal_met_seed_stays_with_capacity() {
        let mut low = seed("low", 10);
        low.active_generation = Some(8);
        let high = seed("high", 20);
        let decisions = TorrentAutoManager::reconcile(&[low.clone(), high.clone()], 3, 1, 500);
        assert!(matches!(
            action(&decisions, "low"),
            AdmissionAction::Stop { .. }
        ));
        assert_eq!(action(&decisions, "high"), AdmissionAction::Start);

        let decisions = TorrentAutoManager::reconcile(&[low, high], 3, 2, 500);
        assert_eq!(
            action(&decisions, "low"),
            AdmissionAction::Retain { generation: 8 }
        );
        assert_eq!(action(&decisions, "high"), AdmissionAction::Start);
    }

    #[test]
    fn inactive_active_torrents_fill_unused_type_capacity() {
        let mut slow_download = download("slow-download", 1);
        slow_download.active_generation = Some(1);
        slow_download.inactive = true;
        let next_download = download("next-download", 2);
        let mut slow_seed = seed("slow-seed", 1);
        slow_seed.active_generation = Some(2);
        slow_seed.inactive = true;
        let next_seed = seed("next-seed", 2);
        let decisions = TorrentAutoManager::reconcile(
            &[slow_download, next_download, slow_seed, next_seed],
            1,
            1,
            4,
        );
        for id in ["slow-download", "next-download", "slow-seed", "next-seed"] {
            assert!(!matches!(action(&decisions, id), AdmissionAction::Idle));
        }
    }

    #[test]
    fn downloads_receive_the_fixed_hard_capacity_before_seeds() {
        let downloads = (0..500)
            .map(|index| download(&format!("download-{index:03}"), i64::from(index)))
            .collect::<Vec<_>>();
        let mut torrents = downloads;
        torrents.push(seed("seed", u32::MAX));
        let decisions = TorrentAutoManager::reconcile(&torrents, 500, 500, 500);
        assert_eq!(action(&decisions, "seed"), AdmissionAction::Idle);
        assert_eq!(
            decisions
                .iter()
                .filter(|decision| decision.action == AdmissionAction::Start)
                .count(),
            500
        );
    }

    #[test]
    fn zero_five_and_unlimited_seed_limits_are_exact_and_ties_are_stable() {
        let torrents = (0..7)
            .rev()
            .map(|index| seed(&format!("seed-{index}"), 42))
            .collect::<Vec<_>>();
        for (limit, expected) in [(0, 0), (5, 5), (500, 7)] {
            let decisions = TorrentAutoManager::reconcile(&torrents, 3, limit, 500);
            let mut active = decisions
                .iter()
                .filter(|decision| decision.action == AdmissionAction::Start)
                .map(|decision| decision.torrent_id.as_str())
                .collect::<Vec<_>>();
            active.sort_unstable();
            assert_eq!(active.len(), expected);
            assert_eq!(
                active,
                (0..expected)
                    .map(|index| format!("seed-{index}"))
                    .collect::<Vec<_>>()
            );
        }
        assert!(decisions_are_seeds(&TorrentAutoManager::reconcile(
            &torrents, 3, 5, 500
        )));
    }

    fn decisions_are_seeds(decisions: &[super::TorrentAdmissionDecision]) -> bool {
        decisions
            .iter()
            .all(|decision| decision.admission_type == TorrentAdmissionType::Seed)
    }
}

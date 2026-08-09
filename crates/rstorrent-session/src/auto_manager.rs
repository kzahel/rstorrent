//! Runtime-independent automatic download admission policy.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TorrentAdmissionState {
    pub(crate) torrent_id: String,
    pub(crate) queue_position: Option<i64>,
    pub(crate) desired_running: bool,
    pub(crate) incomplete: bool,
    pub(crate) checking: bool,
    pub(crate) blocked: bool,
    pub(crate) active_generation: Option<u64>,
}

impl TorrentAdmissionState {
    fn eligible(&self) -> bool {
        self.desired_running
            && self.incomplete
            && self.queue_position.is_some()
            && !self.checking
            && !self.blocked
    }

    fn order(&self) -> (i64, &str) {
        (
            self.queue_position.unwrap_or(i64::MAX),
            self.torrent_id.as_str(),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionStopReason {
    Ineligible,
    LimitReduced,
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
    pub(crate) action: AdmissionAction,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TorrentAutoManager;

impl TorrentAutoManager {
    pub(crate) fn reconcile(
        torrents: &[TorrentAdmissionState],
        effective_limit: usize,
    ) -> Vec<TorrentAdmissionDecision> {
        let mut retained = torrents
            .iter()
            .filter(|torrent| torrent.eligible() && torrent.active_generation.is_some())
            .collect::<Vec<_>>();
        retained.sort_unstable_by_key(|torrent| torrent.order());
        retained.truncate(effective_limit);
        let retained_ids = retained
            .iter()
            .map(|torrent| torrent.torrent_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        let remaining = effective_limit.saturating_sub(retained.len());
        let mut starts = torrents
            .iter()
            .filter(|torrent| torrent.eligible() && torrent.active_generation.is_none())
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|torrent| torrent.order());
        starts.truncate(remaining);
        let start_ids = starts
            .iter()
            .map(|torrent| torrent.torrent_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        torrents
            .iter()
            .map(|torrent| {
                let action = match torrent.active_generation {
                    Some(generation) if retained_ids.contains(torrent.torrent_id.as_str()) => {
                        AdmissionAction::Retain { generation }
                    }
                    Some(generation) => AdmissionAction::Stop {
                        generation,
                        reason: if torrent.eligible() {
                            AdmissionStopReason::LimitReduced
                        } else {
                            AdmissionStopReason::Ineligible
                        },
                    },
                    None if start_ids.contains(torrent.torrent_id.as_str()) => {
                        AdmissionAction::Start
                    }
                    None => AdmissionAction::Idle,
                };
                TorrentAdmissionDecision {
                    torrent_id: torrent.torrent_id.clone(),
                    action,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{AdmissionAction, AdmissionStopReason, TorrentAdmissionState, TorrentAutoManager};

    fn torrent(id: &str, position: i64) -> TorrentAdmissionState {
        TorrentAdmissionState {
            torrent_id: id.to_owned(),
            queue_position: Some(position),
            desired_running: true,
            incomplete: true,
            checking: false,
            blocked: false,
            active_generation: None,
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
    fn starts_in_durable_order_up_to_the_limit() {
        let torrents = [
            torrent("later", 20),
            torrent("first", 10),
            torrent("last", 30),
        ];
        let decisions = TorrentAutoManager::reconcile(&torrents, 2);
        assert_eq!(action(&decisions, "first"), AdmissionAction::Start);
        assert_eq!(action(&decisions, "later"), AdmissionAction::Start);
        assert_eq!(action(&decisions, "last"), AdmissionAction::Idle);
    }

    #[test]
    fn healthy_active_torrents_are_not_preempted_by_a_queue_move() {
        let mut active = torrent("active", 20);
        active.active_generation = Some(7);
        let queued_at_head = torrent("queued", 10);
        let decisions = TorrentAutoManager::reconcile(&[active, queued_at_head], 1);
        assert_eq!(
            action(&decisions, "active"),
            AdmissionAction::Retain { generation: 7 }
        );
        assert_eq!(action(&decisions, "queued"), AdmissionAction::Idle);
    }

    #[test]
    fn shrinking_demotes_the_latest_active_queue_positions() {
        let mut first = torrent("first", 10);
        first.active_generation = Some(1);
        let mut second = torrent("second", 20);
        second.active_generation = Some(2);
        let mut third = torrent("third", 30);
        third.active_generation = Some(3);
        let decisions = TorrentAutoManager::reconcile(&[third, first, second], 1);
        assert_eq!(
            action(&decisions, "first"),
            AdmissionAction::Retain { generation: 1 }
        );
        for (id, generation) in [("second", 2), ("third", 3)] {
            assert_eq!(
                action(&decisions, id),
                AdmissionAction::Stop {
                    generation,
                    reason: AdmissionStopReason::LimitReduced,
                }
            );
        }
    }

    #[test]
    fn ineligible_active_torrents_stop_and_open_capacity() {
        let mut paused = torrent("paused", 10);
        paused.desired_running = false;
        paused.active_generation = Some(4);
        let next = torrent("next", 20);
        let decisions = TorrentAutoManager::reconcile(&[paused, next], 1);
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
    fn checking_and_metadata_acquisition_have_explicit_admission_shapes() {
        let mut checking = torrent("checking", 10);
        checking.checking = true;
        let metadata = torrent("metadata", 20);
        let decisions = TorrentAutoManager::reconcile(&[checking, metadata], 1);
        assert_eq!(action(&decisions, "checking"), AdmissionAction::Idle);
        assert_eq!(action(&decisions, "metadata"), AdmissionAction::Start);
    }
}

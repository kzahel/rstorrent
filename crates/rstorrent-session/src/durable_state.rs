use crate::control::TorrentState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VerificationState {
    requested: u64,
    completed: u64,
}

impl VerificationState {
    pub(crate) fn new(requested: u64, completed: u64) -> Option<Self> {
        (completed <= requested).then_some(Self {
            requested,
            completed,
        })
    }

    pub(crate) const fn requested(self) -> u64 {
        self.requested
    }

    #[cfg(test)]
    pub(crate) const fn completed(self) -> u64 {
        self.completed
    }

    pub(crate) const fn is_pending(self) -> bool {
        self.requested != self.completed
    }

    #[cfg(test)]
    pub(crate) fn request(self) -> Option<Self> {
        if self.is_pending() {
            Some(self)
        } else {
            self.requested.checked_add(1).map(|requested| Self {
                requested,
                completed: self.completed,
            })
        }
    }

    #[cfg(test)]
    pub(crate) const fn complete(self) -> Self {
        Self {
            requested: self.requested,
            completed: self.requested,
        }
    }
}

pub(crate) struct DerivedStateInput {
    pub(crate) metadata_available: bool,
    pub(crate) root_available: bool,
    pub(crate) desired_running: bool,
    pub(crate) has_wanted_pieces: bool,
    pub(crate) verification: VerificationState,
    pub(crate) all_wanted_verified: bool,
    pub(crate) quarantined: bool,
}

pub(crate) fn derive_torrent_state(input: DerivedStateInput) -> TorrentState {
    if input.quarantined {
        TorrentState::NeedsRepair
    } else if !input.metadata_available {
        if input.desired_running {
            TorrentState::AwaitingMetadata
        } else {
            TorrentState::Paused
        }
    } else if !input.has_wanted_pieces {
        TorrentState::Paused
    } else if !input.root_available {
        TorrentState::AwaitingStorage
    } else if input.verification.is_pending() {
        TorrentState::Checking
    } else if !input.desired_running {
        TorrentState::Paused
    } else if input.all_wanted_verified {
        TorrentState::Complete
    } else {
        TorrentState::Downloading
    }
}

#[cfg(test)]
mod tests {
    use super::{DerivedStateInput, VerificationState, derive_torrent_state};
    use crate::control::TorrentState;

    #[test]
    fn verification_requests_are_idempotent_while_pending_and_bounded() {
        let current = VerificationState::new(4, 4).expect("valid state");
        let pending = current.request().expect("request generation");
        assert_eq!(pending.requested(), 5);
        assert_eq!(pending.completed(), 4);
        assert_eq!(pending.request(), Some(pending));
        assert_eq!(pending.complete(), VerificationState::new(5, 5).unwrap());
        assert_eq!(VerificationState::new(4, 5), None);
        assert_eq!(
            VerificationState::new(u64::MAX, u64::MAX)
                .unwrap()
                .request(),
            None
        );
    }

    #[test]
    fn runtime_state_is_derived_in_priority_order() {
        let base = || DerivedStateInput {
            metadata_available: true,
            root_available: true,
            desired_running: true,
            has_wanted_pieces: true,
            verification: VerificationState::new(2, 2).unwrap(),
            all_wanted_verified: true,
            quarantined: false,
        };
        assert_eq!(derive_torrent_state(base()), TorrentState::Complete);

        let mut input = base();
        input.quarantined = true;
        assert_eq!(derive_torrent_state(input), TorrentState::NeedsRepair);
        let mut input = base();
        input.metadata_available = false;
        assert_eq!(derive_torrent_state(input), TorrentState::AwaitingMetadata);
        let mut input = base();
        input.root_available = false;
        assert_eq!(derive_torrent_state(input), TorrentState::AwaitingStorage);
        let mut input = base();
        input.verification = VerificationState::new(3, 2).unwrap();
        assert_eq!(derive_torrent_state(input), TorrentState::Checking);
        let mut input = base();
        input.desired_running = false;
        assert_eq!(derive_torrent_state(input), TorrentState::Paused);
        let mut input = base();
        input.all_wanted_verified = false;
        assert_eq!(derive_torrent_state(input), TorrentState::Downloading);
    }
}

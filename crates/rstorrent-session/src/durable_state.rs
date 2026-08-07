use crate::control::{StorageState, TorrentState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PayloadState {
    Absent,
    LegacyOwned,
    WorkOwned,
    PublicationPending,
    FinalOwned,
}

impl PayloadState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::LegacyOwned => "legacy_owned",
            Self::WorkOwned => "work_owned",
            Self::PublicationPending => "publication_pending",
            Self::FinalOwned => "final_owned",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "absent" => Some(Self::Absent),
            "legacy_owned" => Some(Self::LegacyOwned),
            "work_owned" => Some(Self::WorkOwned),
            "publication_pending" => Some(Self::PublicationPending),
            "final_owned" => Some(Self::FinalOwned),
            _ => None,
        }
    }

    pub(crate) const fn storage_state(self) -> StorageState {
        match self {
            Self::Absent => StorageState::None,
            Self::LegacyOwned | Self::FinalOwned => StorageState::Published,
            Self::WorkOwned => StorageState::Staging,
            Self::PublicationPending => StorageState::Prepared,
        }
    }

    pub(crate) const fn can_recheck(self) -> bool {
        matches!(self, Self::LegacyOwned | Self::WorkOwned | Self::FinalOwned)
    }
}

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

    pub(crate) const fn completed(self) -> u64 {
        self.completed
    }

    pub(crate) const fn is_pending(self) -> bool {
        self.requested != self.completed
    }

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
    pub(crate) payload: PayloadState,
    pub(crate) verification: VerificationState,
    pub(crate) all_wanted_verified: bool,
    pub(crate) quarantined: bool,
}

pub(crate) fn derive_torrent_state(input: DerivedStateInput) -> TorrentState {
    if input.quarantined {
        TorrentState::NeedsRepair
    } else if !input.metadata_available {
        TorrentState::AwaitingMetadata
    } else if !input.root_available {
        TorrentState::AwaitingStorage
    } else if input.verification.is_pending() {
        TorrentState::Checking
    } else if !input.desired_running {
        TorrentState::Paused
    } else if input.payload == PayloadState::PublicationPending {
        TorrentState::AwaitingPublication
    } else if input.all_wanted_verified
        && matches!(
            input.payload,
            PayloadState::LegacyOwned | PayloadState::FinalOwned
        )
    {
        TorrentState::Complete
    } else {
        TorrentState::Downloading
    }
}

#[cfg(test)]
mod tests {
    use super::{DerivedStateInput, PayloadState, VerificationState, derive_torrent_state};
    use crate::control::{StorageState, TorrentState};

    #[test]
    fn payload_record_is_one_closed_ownership_fact() {
        let cases = [
            ("absent", PayloadState::Absent, StorageState::None, false),
            (
                "legacy_owned",
                PayloadState::LegacyOwned,
                StorageState::Published,
                true,
            ),
            (
                "work_owned",
                PayloadState::WorkOwned,
                StorageState::Staging,
                true,
            ),
            (
                "publication_pending",
                PayloadState::PublicationPending,
                StorageState::Prepared,
                false,
            ),
            (
                "final_owned",
                PayloadState::FinalOwned,
                StorageState::Published,
                true,
            ),
        ];
        for (stored, parsed, presented, can_recheck) in cases {
            assert_eq!(PayloadState::parse(stored), Some(parsed));
            assert_eq!(parsed.as_str(), stored);
            assert_eq!(parsed.storage_state(), presented);
            assert_eq!(parsed.can_recheck(), can_recheck);
        }
        assert_eq!(PayloadState::parse("staging_published"), None);
    }

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
            payload: PayloadState::FinalOwned,
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
        input.payload = PayloadState::PublicationPending;
        assert_eq!(
            derive_torrent_state(input),
            TorrentState::AwaitingPublication
        );
        let mut input = base();
        input.all_wanted_verified = false;
        assert_eq!(derive_torrent_state(input), TorrentState::Downloading);
    }
}

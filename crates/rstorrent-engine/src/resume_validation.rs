//! Task-free policy for admitting durable per-torrent resume state.

/// Why the caller is validating durable resume state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeValidationIntent {
    /// Ordinary startup may trust committed pieces after structural checks.
    FastEligible,
    /// A durable verification generation requires a complete hash pass.
    Full,
}

/// A cheap storage observation which the task-free admission policy consumes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeStorageEvidence {
    Matches,
    ContentMismatch(ResumeValidationRejectReason),
    AwaitingStorage,
    NeedsRepair,
}

/// Closed reasons for entering the existing full checker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeValidationRejectReason {
    PendingVerification,
    CreatedStorageWithCommittedPieces,
    MissingPayloadFile,
    UnexpectedPayloadLength,
    MissingPartFile,
    MissingPartSlot,
    TruncatedPartSlot,
}

/// Exact per-torrent outcome of resume admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeAdmissionOutcome {
    Accepted,
    NeedsFullCheck(ResumeValidationRejectReason),
    AwaitingStorage,
    NeedsRepair,
}

/// Combine durable intent and storage evidence without tasks or infrastructure.
pub const fn decide_resume_admission(
    intent: ResumeValidationIntent,
    storage: ResumeStorageEvidence,
) -> ResumeAdmissionOutcome {
    match storage {
        ResumeStorageEvidence::AwaitingStorage => ResumeAdmissionOutcome::AwaitingStorage,
        ResumeStorageEvidence::NeedsRepair => ResumeAdmissionOutcome::NeedsRepair,
        ResumeStorageEvidence::Matches | ResumeStorageEvidence::ContentMismatch(_)
            if matches!(intent, ResumeValidationIntent::Full) =>
        {
            ResumeAdmissionOutcome::NeedsFullCheck(
                ResumeValidationRejectReason::PendingVerification,
            )
        }
        ResumeStorageEvidence::Matches => ResumeAdmissionOutcome::Accepted,
        ResumeStorageEvidence::ContentMismatch(reason) => {
            ResumeAdmissionOutcome::NeedsFullCheck(reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResumeAdmissionOutcome, ResumeStorageEvidence, ResumeValidationIntent,
        ResumeValidationRejectReason, decide_resume_admission,
    };

    #[test]
    fn fast_eligible_matching_storage_is_accepted() {
        assert_eq!(
            decide_resume_admission(
                ResumeValidationIntent::FastEligible,
                ResumeStorageEvidence::Matches,
            ),
            ResumeAdmissionOutcome::Accepted,
        );
    }

    #[test]
    fn content_mismatch_is_torrent_local_full_check() {
        assert_eq!(
            decide_resume_admission(
                ResumeValidationIntent::FastEligible,
                ResumeStorageEvidence::ContentMismatch(
                    ResumeValidationRejectReason::UnexpectedPayloadLength,
                ),
            ),
            ResumeAdmissionOutcome::NeedsFullCheck(
                ResumeValidationRejectReason::UnexpectedPayloadLength,
            ),
        );
    }

    #[test]
    fn pending_verification_overrides_matching_content() {
        assert_eq!(
            decide_resume_admission(ResumeValidationIntent::Full, ResumeStorageEvidence::Matches,),
            ResumeAdmissionOutcome::NeedsFullCheck(
                ResumeValidationRejectReason::PendingVerification,
            ),
        );
    }

    #[test]
    fn unavailable_and_malformed_storage_never_enter_checker() {
        for intent in [
            ResumeValidationIntent::FastEligible,
            ResumeValidationIntent::Full,
        ] {
            assert_eq!(
                decide_resume_admission(intent, ResumeStorageEvidence::AwaitingStorage),
                ResumeAdmissionOutcome::AwaitingStorage,
            );
            assert_eq!(
                decide_resume_admission(intent, ResumeStorageEvidence::NeedsRepair),
                ResumeAdmissionOutcome::NeedsRepair,
            );
        }
    }
}

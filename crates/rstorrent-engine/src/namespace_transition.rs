//! Task-free storage namespace transition policy.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceState {
    None,
    Staging,
    Publishing,
    Published,
}

impl NamespaceState {
    pub const fn initial_generation(self) -> u64 {
        match self {
            Self::None | Self::Staging | Self::Publishing => 0,
            Self::Published => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceAction {
    PreparePublication,
    ConfirmPublication,
    BeginRemoval,
    ConfirmRemoval,
    LoseRoot,
    RepairRoot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceDisposition {
    Apply,
    AlreadyApplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamespaceTransitionInput {
    pub state: NamespaceState,
    pub current_generation: u64,
    pub expected_generation: u64,
    pub action: NamespaceAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamespaceTransitionOutcome {
    pub state: NamespaceState,
    pub generation: u64,
    pub disposition: NamespaceDisposition,
    pub revoke_access: bool,
    pub observation_required: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamespaceTransitionError {
    StaleGeneration {
        expected: u64,
        current: u64,
    },
    InvalidState {
        state: NamespaceState,
        action: NamespaceAction,
    },
    GenerationExhausted,
}

impl fmt::Display for NamespaceTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleGeneration { expected, current } => write!(
                formatter,
                "namespace generation {expected} is stale; current generation is {current}"
            ),
            Self::InvalidState { state, action } => {
                write!(
                    formatter,
                    "namespace action {action:?} is invalid in state {state:?}"
                )
            }
            Self::GenerationExhausted => formatter.write_str("namespace generation exhausted"),
        }
    }
}

impl std::error::Error for NamespaceTransitionError {}

pub fn decide_namespace_transition(
    input: NamespaceTransitionInput,
) -> Result<NamespaceTransitionOutcome, NamespaceTransitionError> {
    if input.expected_generation != input.current_generation {
        return Err(NamespaceTransitionError::StaleGeneration {
            expected: input.expected_generation,
            current: input.current_generation,
        });
    }

    let unchanged = |disposition, revoke_access, observation_required| {
        Ok(NamespaceTransitionOutcome {
            state: input.state,
            generation: input.current_generation,
            disposition,
            revoke_access,
            observation_required,
        })
    };
    let advanced = |state, revoke_access, observation_required| {
        Ok(NamespaceTransitionOutcome {
            state,
            generation: input
                .current_generation
                .checked_add(1)
                .ok_or(NamespaceTransitionError::GenerationExhausted)?,
            disposition: NamespaceDisposition::Apply,
            revoke_access,
            observation_required,
        })
    };

    match (input.action, input.state) {
        (NamespaceAction::PreparePublication, NamespaceState::Staging) => {
            unchanged(NamespaceDisposition::Apply, true, false).map(|mut outcome| {
                outcome.state = NamespaceState::Publishing;
                outcome
            })
        }
        (NamespaceAction::PreparePublication, NamespaceState::Publishing) => {
            unchanged(NamespaceDisposition::AlreadyApplied, true, false)
        }
        (NamespaceAction::ConfirmPublication, NamespaceState::Publishing) => {
            advanced(NamespaceState::Published, true, true)
        }
        (NamespaceAction::ConfirmPublication, NamespaceState::Published) => {
            unchanged(NamespaceDisposition::AlreadyApplied, false, false)
        }
        (NamespaceAction::BeginRemoval, NamespaceState::None) => {
            unchanged(NamespaceDisposition::AlreadyApplied, false, false)
        }
        (NamespaceAction::BeginRemoval, _) => advanced(input.state, true, false),
        (NamespaceAction::ConfirmRemoval, NamespaceState::None) => {
            unchanged(NamespaceDisposition::AlreadyApplied, false, false)
        }
        (NamespaceAction::ConfirmRemoval, _) => unchanged(NamespaceDisposition::Apply, false, true)
            .map(|mut outcome| {
                outcome.state = NamespaceState::None;
                outcome
            }),
        (NamespaceAction::LoseRoot, _) => advanced(input.state, true, false),
        (NamespaceAction::RepairRoot, _) => advanced(input.state, true, true),
        (action, state) => Err(NamespaceTransitionError::InvalidState { state, action }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NamespaceAction, NamespaceDisposition, NamespaceState, NamespaceTransitionError,
        NamespaceTransitionInput, decide_namespace_transition,
    };

    fn decide(
        state: NamespaceState,
        generation: u64,
        action: NamespaceAction,
    ) -> Result<super::NamespaceTransitionOutcome, NamespaceTransitionError> {
        decide_namespace_transition(NamespaceTransitionInput {
            state,
            current_generation: generation,
            expected_generation: generation,
            action,
        })
    }

    #[test]
    fn publication_is_explicit_and_idempotent_only_at_boundaries() {
        let prepared = decide(
            NamespaceState::Staging,
            7,
            NamespaceAction::PreparePublication,
        )
        .expect("prepare publication");
        assert_eq!(prepared.state, NamespaceState::Publishing);
        assert_eq!(prepared.generation, 7);
        assert!(prepared.revoke_access);

        let published = decide(
            prepared.state,
            prepared.generation,
            NamespaceAction::ConfirmPublication,
        )
        .expect("confirm publication");
        assert_eq!(published.state, NamespaceState::Published);
        assert_eq!(published.generation, 8);
        assert!(published.observation_required);

        let replay = decide(
            published.state,
            published.generation,
            NamespaceAction::ConfirmPublication,
        )
        .expect("replay publication confirmation");
        assert_eq!(replay.disposition, NamespaceDisposition::AlreadyApplied);

        assert!(matches!(
            decide(
                NamespaceState::Staging,
                7,
                NamespaceAction::ConfirmPublication,
            ),
            Err(NamespaceTransitionError::InvalidState { .. })
        ));
    }

    #[test]
    fn stale_completion_is_rejected_before_state_changes() {
        let error = decide_namespace_transition(NamespaceTransitionInput {
            state: NamespaceState::Publishing,
            current_generation: 9,
            expected_generation: 8,
            action: NamespaceAction::ConfirmPublication,
        })
        .expect_err("stale completion rejected");
        assert_eq!(
            error,
            NamespaceTransitionError::StaleGeneration {
                expected: 8,
                current: 9,
            }
        );
    }

    #[test]
    fn removal_and_root_changes_fence_access_before_adapter_work() {
        for action in [
            NamespaceAction::BeginRemoval,
            NamespaceAction::LoseRoot,
            NamespaceAction::RepairRoot,
        ] {
            let outcome = decide(NamespaceState::Published, 3, action).expect("transition");
            assert!(outcome.revoke_access);
            assert_eq!(outcome.generation, 4);
            assert_eq!(
                outcome.observation_required,
                action == NamespaceAction::RepairRoot
            );
        }

        let removed = decide(
            NamespaceState::Published,
            4,
            NamespaceAction::ConfirmRemoval,
        )
        .expect("confirm removal");
        assert_eq!(removed.state, NamespaceState::None);
        assert!(removed.observation_required);
    }
}

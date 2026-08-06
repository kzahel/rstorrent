//! Task-free generation fencing for live client-settings reconciliation.

#![allow(
    dead_code,
    reason = "the pure Gate 1 model is wired into the reconciler in Gate 4 of Tactical 097"
)]

use std::array;
use std::error::Error;
use std::fmt;

use super::contract::{ClientSettings, ClientSettingsApplicationState, MAX_RUNTIME_DETAIL_BYTES};
use super::runtime::bounded_utf8;

const DOMAIN_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsDomain {
    Transport,
    PortMapping,
    PeerConnections,
    UploadSlots,
}

impl SettingsDomain {
    const ALL: [Self; DOMAIN_COUNT] = [
        Self::Transport,
        Self::PortMapping,
        Self::PeerConnections,
        Self::UploadSlots,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Transport => 0,
            Self::PortMapping => 1,
            Self::PeerConnections => 2,
            Self::UploadSlots => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SettingsDomainGeneration {
    attempt: u64,
    domain: SettingsDomain,
    generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SettingsAttempt {
    pub(crate) generation: u64,
    pub(crate) settings: ClientSettings,
    domains: [SettingsDomainGeneration; DOMAIN_COUNT],
}

impl SettingsAttempt {
    pub(crate) fn domain(&self, domain: SettingsDomain) -> SettingsDomainGeneration {
        self.domains[domain.index()]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SettingsConvergenceModel {
    next_attempt: u64,
    domain_generations: [u64; DOMAIN_COUNT],
    states: [ClientSettingsApplicationState; DOMAIN_COUNT],
}

impl Default for SettingsConvergenceModel {
    fn default() -> Self {
        Self {
            next_attempt: 0,
            domain_generations: [0; DOMAIN_COUNT],
            states: array::from_fn(|_| ClientSettingsApplicationState::Applied),
        }
    }
}

impl SettingsConvergenceModel {
    pub(crate) fn begin(
        &mut self,
        settings: ClientSettings,
    ) -> Result<SettingsAttempt, SettingsGenerationOverflow> {
        let attempt = next_generation(self.next_attempt)?;
        let mut next_domains = self.domain_generations;
        for generation in &mut next_domains {
            *generation = next_generation(*generation)?;
        }

        self.next_attempt = attempt;
        self.domain_generations = next_domains;
        self.states = array::from_fn(|_| ClientSettingsApplicationState::Applying);
        Ok(SettingsAttempt {
            generation: attempt,
            settings,
            domains: SettingsDomain::ALL.map(|domain| SettingsDomainGeneration {
                attempt,
                domain,
                generation: next_domains[domain.index()],
            }),
        })
    }

    pub(crate) fn apply(
        &mut self,
        generation: SettingsDomainGeneration,
        state: ClientSettingsApplicationState,
    ) -> bool {
        if generation.attempt != self.next_attempt
            || generation.generation != self.domain_generations[generation.domain.index()]
        {
            return false;
        }
        let state = bounded_application_state(state);
        let current = &mut self.states[generation.domain.index()];
        if *current == state {
            return false;
        }
        *current = state;
        true
    }

    pub(crate) fn state(&self, domain: SettingsDomain) -> &ClientSettingsApplicationState {
        &self.states[domain.index()]
    }

    #[cfg(test)]
    fn with_generations_for_testing(attempt: u64, domains: [u64; DOMAIN_COUNT]) -> Self {
        Self {
            next_attempt: attempt,
            domain_generations: domains,
            ..Self::default()
        }
    }
}

fn bounded_application_state(
    state: ClientSettingsApplicationState,
) -> ClientSettingsApplicationState {
    match state {
        ClientSettingsApplicationState::Degraded { reason, detail } => {
            ClientSettingsApplicationState::Degraded {
                reason,
                detail: bounded_utf8(&detail, MAX_RUNTIME_DETAIL_BYTES),
            }
        }
        state => state,
    }
}

fn next_generation(current: u64) -> Result<u64, SettingsGenerationOverflow> {
    current
        .checked_add(1)
        .filter(|generation| *generation != 0)
        .ok_or(SettingsGenerationOverflow)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SettingsGenerationOverflow;

impl fmt::Display for SettingsGenerationOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("client-settings runtime generation exhausted")
    }
}

impl Error for SettingsGenerationOverflow {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientSettingsDegradedReason;

    #[test]
    fn attempts_are_nonzero_and_same_value_retries_get_fresh_generations() {
        let mut model = SettingsConvergenceModel::default();
        let settings = ClientSettings::default();
        let first = model.begin(settings.clone()).unwrap();
        let retry = model.begin(settings).unwrap();
        assert_eq!(first.generation, 1);
        assert_eq!(retry.generation, 2);
        for domain in SettingsDomain::ALL {
            assert_ne!(first.domain(domain), retry.domain(domain));
            assert_eq!(
                model.state(domain),
                &ClientSettingsApplicationState::Applying
            );
        }
    }

    #[test]
    fn domains_converge_independently_and_stale_results_are_rejected() {
        let mut model = SettingsConvergenceModel::default();
        let first = model.begin(ClientSettings::default()).unwrap();
        let replacement = ClientSettings {
            upload_slots: 0,
            ..ClientSettings::default()
        };
        let second = model.begin(replacement).unwrap();

        assert!(!model.apply(
            first.domain(SettingsDomain::Transport),
            ClientSettingsApplicationState::Applied,
        ));
        assert!(model.apply(
            second.domain(SettingsDomain::Transport),
            ClientSettingsApplicationState::Applied,
        ));
        assert!(model.apply(
            second.domain(SettingsDomain::UploadSlots),
            ClientSettingsApplicationState::Degraded {
                reason: ClientSettingsDegradedReason::UploadSlotConvergenceFailed,
                detail: "writer did not stop".to_owned(),
            },
        ));
        assert_eq!(
            model.state(SettingsDomain::PeerConnections),
            &ClientSettingsApplicationState::Applying
        );
        assert_eq!(
            model.state(SettingsDomain::Transport),
            &ClientSettingsApplicationState::Applied
        );
        assert!(matches!(
            model.state(SettingsDomain::UploadSlots),
            ClientSettingsApplicationState::Degraded { .. }
        ));
    }

    #[test]
    fn rapid_a_b_c_changes_only_accept_c_and_bound_failure_detail() {
        let mut model = SettingsConvergenceModel::default();
        let a = model.begin(ClientSettings::default()).unwrap();
        let b = model.begin(ClientSettings::default()).unwrap();
        let c = model.begin(ClientSettings::default()).unwrap();
        let domain = SettingsDomain::PortMapping;
        let degraded = ClientSettingsApplicationState::Degraded {
            reason: ClientSettingsDegradedReason::PortMappingCleanupFailed,
            detail: "é".repeat(400),
        };
        assert!(!model.apply(a.domain(domain), degraded.clone()));
        assert!(!model.apply(b.domain(domain), degraded.clone()));
        assert!(model.apply(c.domain(domain), degraded));
        let ClientSettingsApplicationState::Degraded { detail, .. } = model.state(domain) else {
            panic!("current generation must publish degraded state");
        };
        assert!(detail.len() <= MAX_RUNTIME_DETAIL_BYTES);
        assert!(detail.is_char_boundary(detail.len()));
    }

    #[test]
    fn attempt_and_domain_generation_overflow_never_wraps() {
        let mut attempt_overflow =
            SettingsConvergenceModel::with_generations_for_testing(u64::MAX, [1; DOMAIN_COUNT]);
        assert_eq!(
            attempt_overflow.begin(ClientSettings::default()),
            Err(SettingsGenerationOverflow)
        );

        let mut domain_overflow =
            SettingsConvergenceModel::with_generations_for_testing(1, [1, u64::MAX, 1, 1]);
        assert_eq!(
            domain_overflow.begin(ClientSettings::default()),
            Err(SettingsGenerationOverflow)
        );
        assert_eq!(domain_overflow.next_attempt, 1);
        assert_eq!(domain_overflow.domain_generations[1], u64::MAX);
    }
}

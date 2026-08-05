//! Task-free reachability generation and status transitions.
//!
//! Network ownership lives in the coordinator added by the mapping runtime.
//! This module keeps eligibility and stale-result fencing deterministic.

use std::net::{Ipv4Addr, SocketAddrV4};

use crate::settings::{
    ClientSettings, ListenerPolicy, ListenerStatus, PortMappingFailureStage, PortMappingMechanism,
    PortMappingPolicy, PortMappingStatus,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ReachabilityEvent {
    Discovering,
    Mapping,
    Mapped {
        external_address: Ipv4Addr,
        external_port: u16,
        lease_seconds: u32,
    },
    Failed {
        stage: PortMappingFailureStage,
        detail: String,
    },
    RenewalFailed {
        external_address: Ipv4Addr,
        external_port: u16,
        detail: String,
    },
    Stopping,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReachabilityState {
    generation: u64,
    local_endpoint: Option<SocketAddrV4>,
    status: PortMappingStatus,
    stopping: bool,
}

impl ReachabilityState {
    pub(crate) fn new(
        generation: u64,
        settings: &ClientSettings,
        listener_status: &ListenerStatus,
    ) -> Self {
        let local_endpoint = eligible_local_endpoint(settings, listener_status);
        let status = if settings.port_mapping == PortMappingPolicy::Disabled {
            PortMappingStatus::Disabled
        } else if local_endpoint.is_some() {
            PortMappingStatus::Discovering
        } else {
            PortMappingStatus::Ineligible
        };
        Self {
            generation,
            local_endpoint,
            status,
            stopping: false,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn local_endpoint(&self) -> Option<SocketAddrV4> {
        self.local_endpoint
    }

    pub(crate) fn status(&self) -> &PortMappingStatus {
        &self.status
    }

    pub(crate) fn apply(&mut self, generation: u64, event: ReachabilityEvent) -> bool {
        if generation != self.generation || self.stopping {
            return false;
        }
        let Some(local_endpoint) = self.local_endpoint else {
            return false;
        };
        let next = match event {
            ReachabilityEvent::Discovering => PortMappingStatus::Discovering,
            ReachabilityEvent::Mapping if matches!(self.status, PortMappingStatus::Discovering) => {
                PortMappingStatus::Mapping
            }
            ReachabilityEvent::Mapped {
                external_address,
                external_port,
                lease_seconds,
            } if matches!(
                self.status,
                PortMappingStatus::Mapping
                    | PortMappingStatus::Mapped { .. }
                    | PortMappingStatus::RenewalFailed { .. }
            ) =>
            {
                PortMappingStatus::Mapped {
                    mechanism: PortMappingMechanism::UpnpIgdV2,
                    local_address: local_endpoint.ip().to_string(),
                    local_port: local_endpoint.port(),
                    external_address: external_address.to_string(),
                    external_port,
                    lease_seconds,
                }
            }
            ReachabilityEvent::Failed { stage, detail } => {
                PortMappingStatus::Failed { stage, detail }
            }
            ReachabilityEvent::RenewalFailed {
                external_address,
                external_port,
                detail,
            } if matches!(self.status, PortMappingStatus::Mapped { .. }) => {
                PortMappingStatus::RenewalFailed {
                    external_address: external_address.to_string(),
                    external_port,
                    detail,
                }
            }
            ReachabilityEvent::Stopping => {
                self.stopping = true;
                PortMappingStatus::Stopping
            }
            _ => return false,
        };
        if self.status == next {
            return false;
        }
        self.status = next;
        true
    }
}

fn eligible_local_endpoint(
    settings: &ClientSettings,
    listener_status: &ListenerStatus,
) -> Option<SocketAddrV4> {
    if settings.port_mapping != PortMappingPolicy::Upnp
        || !matches!(
            settings.listener,
            ListenerPolicy::AutomaticLocalNetwork | ListenerPolicy::FixedLocalNetwork { .. }
        )
    {
        return None;
    }
    let ListenerStatus::Listening { address, port } = listener_status else {
        return None;
    };
    let address = address.parse::<Ipv4Addr>().ok()?;
    if *port == 0
        || address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || address == Ipv4Addr::BROADCAST
    {
        return None;
    }
    Some(SocketAddrV4::new(address, *port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eligible_settings() -> ClientSettings {
        ClientSettings {
            listener: ListenerPolicy::AutomaticLocalNetwork,
            port_mapping: PortMappingPolicy::Upnp,
            ..ClientSettings::default()
        }
    }

    fn listening() -> ListenerStatus {
        ListenerStatus::Listening {
            address: "192.168.50.12".to_owned(),
            port: 41_234,
        }
    }

    #[test]
    fn policy_and_observed_listener_jointly_determine_eligibility() {
        let disabled = ReachabilityState::new(1, &ClientSettings::default(), &listening());
        assert_eq!(disabled.status(), &PortMappingStatus::Disabled);

        let loopback = ClientSettings {
            listener: ListenerPolicy::AutomaticLoopback,
            port_mapping: PortMappingPolicy::Upnp,
            ..ClientSettings::default()
        };
        assert_eq!(
            ReachabilityState::new(2, &loopback, &listening()).status(),
            &PortMappingStatus::Ineligible
        );
        assert_eq!(
            ReachabilityState::new(
                3,
                &eligible_settings(),
                &ListenerStatus::Listening {
                    address: "127.0.0.1".to_owned(),
                    port: 41_234,
                },
            )
            .status(),
            &PortMappingStatus::Ineligible
        );

        let eligible = ReachabilityState::new(4, &eligible_settings(), &listening());
        assert_eq!(eligible.status(), &PortMappingStatus::Discovering);
        assert_eq!(
            eligible.local_endpoint(),
            Some(SocketAddrV4::new(Ipv4Addr::new(192, 168, 50, 12), 41_234))
        );
    }

    #[test]
    fn transitions_are_ordered_and_stale_generations_are_fenced() {
        let mut state = ReachabilityState::new(7, &eligible_settings(), &listening());
        assert!(!state.apply(6, ReachabilityEvent::Mapping));
        assert!(!state.apply(
            7,
            ReachabilityEvent::Mapped {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                lease_seconds: 3_600,
            },
        ));
        assert!(state.apply(7, ReachabilityEvent::Mapping));
        assert!(state.apply(
            7,
            ReachabilityEvent::Mapped {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                lease_seconds: 3_600,
            },
        ));
        assert!(state.apply(
            7,
            ReachabilityEvent::RenewalFailed {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                detail: "gateway did not answer".to_owned(),
            },
        ));
        assert!(state.apply(
            7,
            ReachabilityEvent::Mapped {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                lease_seconds: 3_600,
            },
        ));
        assert!(state.apply(7, ReachabilityEvent::Stopping));
        assert!(!state.apply(7, ReachabilityEvent::Discovering));
        assert_eq!(state.status(), &PortMappingStatus::Stopping);
        assert_eq!(state.generation(), 7);
    }
}

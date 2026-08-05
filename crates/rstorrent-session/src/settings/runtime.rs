use std::io;

use rstorrent_engine::{IncomingTcpBootstrap, PeerBudgetConfig, UploadSchedulerConfig};

use super::contract::{
    ClientSettings, ClientSettingsRuntimeView, ListenerBindFailureReason, ListenerPolicy,
    ListenerStatus, MAX_LISTENER_BIND_DETAIL_BYTES, PortMappingStatus, SessionUdpStatus,
};
use crate::reachability::ReachabilityState;

impl ClientSettings {
    pub(crate) fn incoming_bootstrap(&self) -> IncomingTcpBootstrap {
        match self.listener {
            ListenerPolicy::Disabled => IncomingTcpBootstrap::Disabled,
            ListenerPolicy::AutomaticLoopback => IncomingTcpBootstrap::AutomaticLoopback,
            ListenerPolicy::FixedLoopback { port } => IncomingTcpBootstrap::FixedLoopback(port),
            ListenerPolicy::AutomaticLocalNetwork => IncomingTcpBootstrap::AutomaticLocalNetwork,
            ListenerPolicy::FixedLocalNetwork { port } => {
                IncomingTcpBootstrap::FixedLocalNetwork(port)
            }
        }
    }

    pub(crate) fn peer_budget_config(&self) -> PeerBudgetConfig {
        PeerBudgetConfig {
            configured_limit: usize::try_from(self.peer_connection_limit)
                .expect("u32 peer limit fits usize on supported targets"),
            ..PeerBudgetConfig::system_default()
        }
    }

    pub(crate) fn upload_scheduler_config(&self) -> UploadSchedulerConfig {
        UploadSchedulerConfig {
            slots: usize::from(self.upload_slots),
            ..UploadSchedulerConfig::default()
        }
    }
}

impl ClientSettingsRuntimeView {
    pub(crate) fn from_configured(settings: ClientSettings) -> Self {
        Self {
            effective_peer_connection_limit: settings.peer_connection_limit,
            configured: settings.clone(),
            active: settings,
            restart_required: false,
            listener_status: ListenerStatus::Disabled,
            session_udp_status: SessionUdpStatus::Unavailable,
            port_mapping_status: PortMappingStatus::Disabled,
        }
    }

    pub(crate) fn set_configured(&mut self, configured: ClientSettings) {
        self.restart_required = configured != self.active;
        self.configured = configured;
    }

    pub(crate) fn from_started(
        configured: ClientSettings,
        active: ClientSettings,
        effective_peer_connection_limit: u32,
        listener_status: ListenerStatus,
        session_udp_status: SessionUdpStatus,
    ) -> Self {
        let port_mapping_status = ReachabilityState::new(1, &active, &listener_status)
            .status()
            .clone();
        Self {
            restart_required: configured != active,
            configured,
            active,
            effective_peer_connection_limit,
            listener_status,
            session_udp_status,
            port_mapping_status,
        }
    }

    pub(crate) fn set_port_mapping_status(&mut self, status: PortMappingStatus) {
        self.port_mapping_status = status;
    }
}

pub(crate) fn classify_listener_bind_failure(error: &io::Error) -> ListenerStatus {
    let reason = match error.kind() {
        io::ErrorKind::AddrInUse => ListenerBindFailureReason::AddressInUse,
        io::ErrorKind::PermissionDenied => ListenerBindFailureReason::PermissionDenied,
        io::ErrorKind::AddrNotAvailable => ListenerBindFailureReason::AddressUnavailable,
        _ => ListenerBindFailureReason::Other,
    };
    ListenerStatus::BindFailed {
        reason,
        detail: bounded_utf8(&error.to_string(), MAX_LISTENER_BIND_DETAIL_BYTES),
    }
}

fn bounded_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

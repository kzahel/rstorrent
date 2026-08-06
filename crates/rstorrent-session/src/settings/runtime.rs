use std::io;

use rstorrent_engine::{IncomingTcpBootstrap, PeerBudgetConfig, UploadSchedulerConfig};

use super::contract::{
    AdvertisedPeerEndpointStatus, ClientSettings, ClientSettingsApplicationState,
    ClientSettingsRuntimeView, EffectiveListenerSettings, ListenerBindFailureReason,
    ListenerPolicy, ListenerStatus, MAX_RUNTIME_DETAIL_BYTES, PortMappingStatus, SessionUdpStatus,
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
            effective_listener: Some(EffectiveListenerSettings::from_settings(&settings)),
            effective_port_mapping: settings.port_mapping,
            effective_peer_connection_limit: settings.peer_connection_limit,
            effective_upload_slots: settings.upload_slots,
            configured: settings.clone(),
            transport_application: ClientSettingsApplicationState::Applied,
            port_mapping_application: ClientSettingsApplicationState::Applied,
            peer_connections_application: ClientSettingsApplicationState::Applied,
            upload_slots_application: ClientSettingsApplicationState::Applied,
            listener_status: ListenerStatus::Disabled,
            session_udp_status: SessionUdpStatus::Unavailable,
            port_mapping_status: PortMappingStatus::Disabled,
            advertised_peer_endpoint: AdvertisedPeerEndpointStatus::Unavailable,
        }
    }

    pub(crate) fn set_configured(&mut self, configured: ClientSettings) {
        if self.configured == configured {
            return;
        }
        self.configured = configured;
        self.transport_application = ClientSettingsApplicationState::Applying;
        self.port_mapping_application = ClientSettingsApplicationState::Applying;
        self.peer_connections_application = ClientSettingsApplicationState::Applying;
        self.upload_slots_application = ClientSettingsApplicationState::Applying;
    }

    pub(crate) fn from_started(
        configured: ClientSettings,
        active: ClientSettings,
        effective_peer_connection_limit: u32,
        listener_status: ListenerStatus,
        session_udp_status: SessionUdpStatus,
        advertised_peer_endpoint: AdvertisedPeerEndpointStatus,
    ) -> Self {
        let port_mapping_status = ReachabilityState::new(1, &active, &listener_status)
            .status()
            .clone();
        Self {
            configured,
            effective_listener: if listener_status == ListenerStatus::Disabled
                || matches!(listener_status, ListenerStatus::Listening { .. })
            {
                Some(EffectiveListenerSettings::from_settings(&active))
            } else {
                None
            },
            effective_port_mapping: active.port_mapping,
            effective_peer_connection_limit,
            effective_upload_slots: active.upload_slots,
            transport_application: if matches!(listener_status, ListenerStatus::BindFailed { .. }) {
                ClientSettingsApplicationState::Degraded {
                    reason: super::contract::ClientSettingsDegradedReason::TransportBindFailed,
                    detail: match &listener_status {
                        ListenerStatus::BindFailed { detail, .. } => detail.clone(),
                        _ => unreachable!("matched listener bind failure"),
                    },
                }
            } else {
                ClientSettingsApplicationState::Applied
            },
            port_mapping_application: ClientSettingsApplicationState::Applied,
            peer_connections_application: ClientSettingsApplicationState::Applied,
            upload_slots_application: ClientSettingsApplicationState::Applied,
            listener_status,
            session_udp_status,
            port_mapping_status,
            advertised_peer_endpoint,
        }
    }

    pub(crate) fn set_port_mapping_status(&mut self, status: PortMappingStatus) {
        self.port_mapping_status = status;
    }

    pub(crate) fn set_advertised_peer_endpoint(&mut self, status: AdvertisedPeerEndpointStatus) {
        self.advertised_peer_endpoint = status;
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
        detail: bounded_utf8(&error.to_string(), MAX_RUNTIME_DETAIL_BYTES),
    }
}

pub(crate) fn bounded_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

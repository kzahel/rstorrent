use rstorrent_engine::{IncomingTcpBootstrap, PeerBudgetConfig, UploadSchedulerConfig};

use super::contract::{ClientSettings, ClientSettingsRuntimeView, ListenerPolicy, ListenerStatus};

impl ClientSettings {
    pub(crate) fn incoming_bootstrap(&self) -> IncomingTcpBootstrap {
        match self.listener {
            ListenerPolicy::Disabled => IncomingTcpBootstrap::Disabled,
            ListenerPolicy::AutomaticLoopback => IncomingTcpBootstrap::AutomaticLoopback,
            ListenerPolicy::FixedLoopback { port } => IncomingTcpBootstrap::FixedLoopback(port),
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
        }
    }

    pub(crate) fn set_configured(&mut self, configured: ClientSettings) {
        self.restart_required = configured != self.active;
        self.configured = configured;
    }
}

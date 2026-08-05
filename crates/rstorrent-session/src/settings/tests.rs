use rstorrent_engine::{
    DEFAULT_CONNECTION_LIMIT, DEFAULT_INCOMING_CONNECTION_SLACK, DEFAULT_UNCHOKE_SLOTS,
};

use super::{
    ClientSettings, ClientSettingsError, ClientSettingsRuntimeView, ListenerPolicy, ListenerStatus,
};

#[test]
fn defaults_follow_engine_policy_without_enabling_the_listener() {
    let settings = ClientSettings::default();
    assert_eq!(settings.listener, ListenerPolicy::Disabled);
    assert_eq!(
        settings.peer_connection_limit,
        u32::try_from(DEFAULT_CONNECTION_LIMIT).unwrap()
    );
    assert_eq!(
        settings.upload_slots,
        u16::try_from(DEFAULT_UNCHOKE_SLOTS).unwrap()
    );
    assert_eq!(settings.validate(), Ok(()));

    let peer_budget = settings.peer_budget_config();
    assert_eq!(peer_budget.configured_limit, DEFAULT_CONNECTION_LIMIT);
    assert_eq!(
        peer_budget.incoming_slack,
        DEFAULT_INCOMING_CONNECTION_SLACK
    );
    assert_eq!(
        settings.upload_scheduler_config().slots,
        DEFAULT_UNCHOKE_SLOTS
    );
}

#[test]
fn validates_exact_listener_connection_and_slot_boundaries() {
    for port in [1_024, 65_535] {
        assert_eq!(
            ClientSettings {
                listener: ListenerPolicy::FixedLoopback { port },
                ..ClientSettings::default()
            }
            .validate(),
            Ok(())
        );
    }
    assert_eq!(
        ClientSettings {
            listener: ListenerPolicy::FixedLoopback { port: 1_023 },
            ..ClientSettings::default()
        }
        .validate(),
        Err(ClientSettingsError::FixedListenerPort { port: 1_023 })
    );

    for value in [1, 2_000] {
        assert_eq!(
            ClientSettings {
                peer_connection_limit: value,
                ..ClientSettings::default()
            }
            .validate(),
            Ok(())
        );
    }
    for value in [0, 2_001] {
        assert_eq!(
            ClientSettings {
                peer_connection_limit: value,
                ..ClientSettings::default()
            }
            .validate(),
            Err(ClientSettingsError::PeerConnectionLimit { value })
        );
    }

    for value in [0, 50] {
        assert_eq!(
            ClientSettings {
                upload_slots: value,
                ..ClientSettings::default()
            }
            .validate(),
            Ok(())
        );
    }
    assert_eq!(
        ClientSettings {
            upload_slots: 51,
            ..ClientSettings::default()
        }
        .validate(),
        Err(ClientSettingsError::UploadSlots { value: 51 })
    );
}

#[test]
fn listener_json_is_closed_and_tagged() {
    let fixed = ListenerPolicy::FixedLoopback { port: 6_881 };
    assert_eq!(
        serde_json::to_value(fixed).unwrap(),
        serde_json::json!({"type": "fixed_loopback", "port": 6881})
    );
    assert!(
        serde_json::from_value::<ListenerPolicy>(
            serde_json::json!({"type": "fixed_loopback", "port": 65536})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ListenerPolicy>(serde_json::json!({"type": "public"})).is_err()
    );
}

#[test]
fn runtime_view_distinguishes_configured_active_effective_and_observed() {
    let active = ClientSettings::default();
    let configured = ClientSettings {
        listener: ListenerPolicy::AutomaticLoopback,
        peer_connection_limit: 500,
        upload_slots: 1,
    };
    let view = ClientSettingsRuntimeView {
        configured: configured.clone(),
        active: active.clone(),
        restart_required: true,
        effective_peer_connection_limit: 120,
        listener_status: ListenerStatus::Listening {
            address: "127.0.0.1".to_owned(),
            port: 41_000,
        },
    };
    assert_eq!(view.configured, configured);
    assert_eq!(view.active, active);
    assert!(view.restart_required);
    assert_eq!(view.effective_peer_connection_limit, 120);
    assert_eq!(
        view.listener_status,
        ListenerStatus::Listening {
            address: "127.0.0.1".to_owned(),
            port: 41_000,
        }
    );
}

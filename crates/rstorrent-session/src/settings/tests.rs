use std::io;

use rstorrent_engine::{
    DEFAULT_CONNECTION_LIMIT, DEFAULT_INCOMING_CONNECTION_SLACK, DEFAULT_UNCHOKE_SLOTS,
};
use rusqlite::Connection;

use super::{
    AdvertisedPeerEndpointScope, AdvertisedPeerEndpointStatus, ClientSettings, ClientSettingsError,
    ClientSettingsRuntimeView, ListenerPolicy, ListenerStatus, PortMappingPolicy,
    PortMappingStatus, SessionUdpStatus, SettingsPersistenceError, classify_listener_bind_failure,
    create_client_settings, read_client_settings, replace_client_settings,
};

#[test]
fn defaults_follow_engine_policy_without_enabling_the_listener() {
    let settings = ClientSettings::default();
    assert_eq!(settings.listener, ListenerPolicy::Disabled);
    assert_eq!(settings.preferred_listen_port, 6_881);
    assert_eq!(settings.port_mapping, PortMappingPolicy::Disabled);
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
    for port in [1_024, 6_881, 65_535] {
        assert_eq!(
            ClientSettings {
                preferred_listen_port: port,
                ..ClientSettings::default()
            }
            .validate(),
            Ok(())
        );
    }
    assert_eq!(
        ClientSettings {
            preferred_listen_port: 1_023,
            ..ClientSettings::default()
        }
        .validate(),
        Err(ClientSettingsError::PreferredListenerPort { port: 1_023 })
    );

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

    for value in [1, 199, 200, 2_000] {
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

    for value in [0, 1, 7, 8, 50] {
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
    assert_eq!(
        serde_json::to_value(ListenerPolicy::AutomaticLocalNetwork).unwrap(),
        serde_json::json!({"type": "automatic_local_network"})
    );
    assert_eq!(
        serde_json::to_value(ListenerPolicy::FixedLocalNetwork { port: 6_882 }).unwrap(),
        serde_json::json!({"type": "fixed_local_network", "port": 6882})
    );
    assert_eq!(
        serde_json::to_value(PortMappingPolicy::Upnp).unwrap(),
        serde_json::json!("upnp")
    );
}

#[test]
fn runtime_view_distinguishes_configured_active_effective_and_observed() {
    let active = ClientSettings::default();
    let configured = ClientSettings {
        listener: ListenerPolicy::AutomaticLoopback,
        preferred_listen_port: 42_000,
        port_mapping: PortMappingPolicy::Disabled,
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
        session_udp_status: SessionUdpStatus::Bound {
            address: "127.0.0.1".to_owned(),
            port: 41_001,
            coordinated_with_tcp: false,
        },
        port_mapping_status: PortMappingStatus::Disabled,
        advertised_peer_endpoint: AdvertisedPeerEndpointStatus::Local {
            generation: "1".to_owned(),
            address: "127.0.0.1".to_owned(),
            port: 41_000,
            scope: AdvertisedPeerEndpointScope::Loopback,
            incoming_observed: false,
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

#[test]
fn listener_bind_failures_are_closed_classified_and_byte_bounded() {
    for (kind, expected) in [
        (
            io::ErrorKind::AddrInUse,
            crate::ListenerBindFailureReason::AddressInUse,
        ),
        (
            io::ErrorKind::PermissionDenied,
            crate::ListenerBindFailureReason::PermissionDenied,
        ),
        (
            io::ErrorKind::AddrNotAvailable,
            crate::ListenerBindFailureReason::AddressUnavailable,
        ),
        (
            io::ErrorKind::ConnectionRefused,
            crate::ListenerBindFailureReason::Other,
        ),
    ] {
        let ListenerStatus::BindFailed { reason, detail } =
            classify_listener_bind_failure(&io::Error::new(kind, "bounded detail"))
        else {
            panic!("bind failure must remain typed");
        };
        assert_eq!(reason, expected);
        assert_eq!(detail, "bounded detail");
    }

    let ListenerStatus::BindFailed { detail, .. } =
        classify_listener_bind_failure(&io::Error::other("é".repeat(400)))
    else {
        panic!("long bind failure must remain typed");
    };
    assert!(detail.len() <= 512);
    assert!(detail.is_char_boundary(detail.len()));
}

#[test]
fn typed_persistence_round_trips_one_atomic_group() {
    let mut connection = Connection::open_in_memory().unwrap();
    let transaction = connection.transaction().unwrap();
    create_client_settings(&transaction).unwrap();
    assert_eq!(
        read_client_settings(&transaction).unwrap(),
        ClientSettings::default()
    );
    let configured = ClientSettings {
        listener: ListenerPolicy::FixedLocalNetwork { port: 42_000 },
        preferred_listen_port: 41_000,
        port_mapping: PortMappingPolicy::Upnp,
        peer_connection_limit: 1,
        upload_slots: 0,
    };
    assert!(replace_client_settings(&transaction, &configured).unwrap());
    assert!(!replace_client_settings(&transaction, &configured).unwrap());
    transaction.commit().unwrap();
    assert_eq!(read_client_settings(&connection).unwrap(), configured);
}

#[test]
fn version_nine_settings_migrate_without_enabling_mapping() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE client_settings (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                listener_mode TEXT NOT NULL,
                listener_port INTEGER,
                peer_connection_limit INTEGER NOT NULL,
                upload_slots INTEGER NOT NULL
             );
             INSERT INTO client_settings VALUES
                (1, 'fixed_loopback', 42001, 321, 3);",
        )
        .unwrap();
    let transaction = connection.transaction().unwrap();
    super::migrate_client_settings_to_v10(&transaction).unwrap();
    super::migrate_client_settings_to_v11(&transaction).unwrap();
    transaction.commit().unwrap();
    assert_eq!(
        read_client_settings(&connection).unwrap(),
        ClientSettings {
            listener: ListenerPolicy::FixedLoopback { port: 42_001 },
            preferred_listen_port: 6_881,
            port_mapping: PortMappingPolicy::Disabled,
            peer_connection_limit: 321,
            upload_slots: 3,
        }
    );
}

#[test]
fn version_ten_settings_migrate_with_the_preferred_port_default() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE client_settings (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                listener_mode TEXT NOT NULL,
                listener_port INTEGER,
                port_mapping_mode TEXT NOT NULL,
                peer_connection_limit INTEGER NOT NULL,
                upload_slots INTEGER NOT NULL
             );
             INSERT INTO client_settings VALUES
                (1, 'automatic_local_network', NULL, 'upnp', 444, 5);",
        )
        .unwrap();
    let transaction = connection.transaction().unwrap();
    super::migrate_client_settings_to_v11(&transaction).unwrap();
    transaction.commit().unwrap();
    assert_eq!(
        read_client_settings(&connection).unwrap(),
        ClientSettings {
            listener: ListenerPolicy::AutomaticLocalNetwork,
            preferred_listen_port: 6_881,
            port_mapping: PortMappingPolicy::Upnp,
            peer_connection_limit: 444,
            upload_slots: 5,
        }
    );
}

#[test]
fn sqlite_constraints_and_decoder_reject_invalid_durable_shapes() {
    let mut connection = Connection::open_in_memory().unwrap();
    let transaction = connection.transaction().unwrap();
    create_client_settings(&transaction).unwrap();
    transaction.commit().unwrap();

    assert!(
        connection
            .execute(
                "UPDATE client_settings SET peer_connection_limit = 0 WHERE singleton = 1",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE client_settings SET preferred_listen_port = 1023 WHERE singleton = 1",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE client_settings SET port_mapping_mode = 'automatic' WHERE singleton = 1",
                [],
            )
            .is_err()
    );
    connection
        .pragma_update(None, "ignore_check_constraints", true)
        .unwrap();
    connection
        .execute(
            "UPDATE client_settings
             SET listener_mode = 'fixed_loopback', listener_port = NULL
             WHERE singleton = 1",
            [],
        )
        .unwrap();
    assert!(matches!(
        read_client_settings(&connection),
        Err(SettingsPersistenceError::Corrupt(_))
    ));
    connection
        .execute("DELETE FROM client_settings", [])
        .unwrap();
    assert!(matches!(
        read_client_settings(&connection),
        Err(SettingsPersistenceError::Corrupt(_))
    ));
}

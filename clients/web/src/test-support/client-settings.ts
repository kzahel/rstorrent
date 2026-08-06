import type { ClientSettings, ClientSettingsRuntimeView } from "../api";

export function clientSettingsFixture(): ClientSettings {
  return {
    listener: { type: "disabled" },
    preferred_listen_port: 6_881,
    port_mapping: "disabled",
    peer_connection_limit: 200,
    upload_slots: 8,
  };
}

export function clientSettingsRuntimeFixture(): ClientSettingsRuntimeView {
  return {
    configured: clientSettingsFixture(),
    effective_listener: {
      listener: { type: "disabled" },
      preferred_listen_port: 6_881,
    },
    effective_port_mapping: "disabled",
    effective_peer_connection_limit: 200,
    effective_upload_slots: 8,
    transport_application: { type: "applied" },
    port_mapping_application: { type: "applied" },
    peer_connections_application: { type: "applied" },
    upload_slots_application: { type: "applied" },
    listener_status: { type: "disabled" },
    session_udp_status: { type: "unavailable" },
    port_mapping_status: { type: "disabled" },
    advertised_peer_endpoint: { type: "unavailable" },
  };
}

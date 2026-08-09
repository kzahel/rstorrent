import type { ClientSettings, ClientSettingsRuntimeView } from "../api";

export function clientSettingsFixture(): ClientSettings {
  return {
    listener: { type: "disabled" },
    preferred_listen_port: 6_881,
    port_mapping: "disabled",
    peer_connection_limit: 200,
    upload_slots: 8,
    active_downloads: 3,
    encryption: "allow",
    ipv6_enabled: true,
    tracker_https_server_authentication: "system_trust",
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
    effective_active_downloads: 3,
    active_download_count: 0,
    checking_count: 0,
    effective_encryption: "allow",
    effective_ipv6_enabled: true,
    effective_tracker_https_server_authentication: "system_trust",
    transport_application: { type: "applied" },
    port_mapping_application: { type: "applied" },
    peer_connections_application: { type: "applied" },
    upload_slots_application: { type: "applied" },
    encryption_application: { type: "applied" },
    ipv6_application: { type: "applied" },
    tracker_https_authentication_application: { type: "applied" },
    listener_status: { type: "disabled" },
    session_udp_status: { type: "unavailable" },
    port_mapping_status: { type: "disabled" },
    ipv6_pinhole_status: { type: "disabled" },
    advertised_peer_endpoint: { type: "unavailable" },
    transport_families: [
      {
        family: "ipv4",
        configured: true,
        tcp_endpoint: null,
        udp_endpoint: null,
        advertised_endpoint: null,
      },
      {
        family: "ipv6",
        configured: true,
        tcp_endpoint: null,
        udp_endpoint: null,
        advertised_endpoint: null,
      },
    ],
  };
}

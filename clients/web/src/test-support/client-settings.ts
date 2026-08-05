import type { ClientSettings, ClientSettingsRuntimeView } from "../api";

export function clientSettingsFixture(): ClientSettings {
  return {
    listener: { type: "disabled" },
    port_mapping: "disabled",
    peer_connection_limit: 200,
    upload_slots: 8,
  };
}

export function clientSettingsRuntimeFixture(): ClientSettingsRuntimeView {
  return {
    configured: clientSettingsFixture(),
    active: clientSettingsFixture(),
    restart_required: false,
    effective_peer_connection_limit: 200,
    listener_status: { type: "disabled" },
    port_mapping_status: { type: "disabled" },
  };
}

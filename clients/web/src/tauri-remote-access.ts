import { invoke } from "@tauri-apps/api/core";

import type {
  DesktopRemoteAccess,
  DesktopRemoteAccessState,
  DisableRemoteAccessOutcome,
  RemoteSecurityView,
} from "./inspection/remote-access/types";

export function createTauriDesktopRemoteAccess(): DesktopRemoteAccess {
  return {
    scope: "local",
    state: () =>
      invoke<DesktopRemoteAccessState>("desktop_remote_access_state"),
    enable: (username, passphrase) =>
      invoke<RemoteSecurityView>("desktop_remote_access_enable", {
        username,
        passphrase,
      }),
    rename: (clientId, label) =>
      invoke("desktop_remote_access_rename", { clientId, label }),
    revoke: (clientId) =>
      invoke("desktop_remote_access_revoke", { clientId }),
    revokeAllOther: (retainedClientId) =>
      invoke<number>("desktop_remote_access_revoke_all_other", {
        retainedClientId,
      }),
    closeCircuit: (circuitId) =>
      invoke("desktop_remote_access_close_circuit", { circuitId }),
    requirePasswordEverywhere: () =>
      invoke<number>("desktop_remote_access_require_password"),
    changePassphrase: (passphrase) =>
      invoke<number>("desktop_remote_access_change_passphrase", {
        passphrase,
      }),
    disable: () =>
      invoke<DisableRemoteAccessOutcome>("desktop_remote_access_disable"),
    recover: (username, passphrase) =>
      invoke<RemoteSecurityView>("desktop_remote_access_recover", {
        username,
        passphrase,
      }),
    clearHistory: () =>
      invoke<boolean>("desktop_remote_access_clear_history"),
  };
}

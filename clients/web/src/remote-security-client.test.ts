import { describe, expect, it, vi } from "vitest";

import type { ApplicationViewClient } from "./api/client";
import type {
  RemoteApplicationWebSocket,
  RemoteControlOperation,
  RemoteControlOutcome,
} from "./remote-application-websocket";
import type { RemoteClientStore } from "./remote-client-store";
import { createRemoteSecurityClient } from "./remote-security-client";

describe("authenticated remote security client", () => {
  it("maps owner audit and mutation operations onto encrypted control records", async () => {
    const operations: RemoteControlOperation[] = [];
    const transport = {
      remoteControl: async (operation: RemoteControlOperation) => {
        operations.push(operation);
        return result(operation);
      },
    } as RemoteApplicationWebSocket;
    const client = createRemoteSecurityClient({
      username: "alice",
      currentClientId: "current-client",
      transport: () => transport,
      application: { close: async () => undefined } as ApplicationViewClient,
      store: store(),
    });
    const state = await client.state();
    expect(state.security?.enabled).toBe(true);
    await client.rename("client-one", "Renamed");
    await client.revoke("client-two");
    expect(await client.revokeAllOther("client-one")).toBe(2);
    expect(await client.requirePasswordEverywhere()).toBe(2);
    expect(operations).toEqual([
      { type: "inspect" },
      { type: "rename", client_id: "client-one", label: "Renamed" },
      { type: "revoke", client_id: "client-two" },
      { type: "revoke_all_other", retained_client_id: "client-one" },
      { type: "require_password_everywhere" },
    ]);
  });

  it("clears only local resume authority when signing out this browser", async () => {
    const clearAuthorization = vi.fn(async () => undefined);
    const close = vi.fn(async () => undefined);
    const reload = vi.fn();
    const client = createRemoteSecurityClient({
      username: "alice",
      transport: () =>
        ({
          remoteControl: async () => ({
            type: "signed_out",
            authorization_revoked: true,
          }),
        }) as unknown as RemoteApplicationWebSocket,
      application: { close } as unknown as ApplicationViewClient,
      store: store({ clearAuthorization }),
      reload,
    });
    await client.signOutThisBrowser?.();
    expect(clearAuthorization).toHaveBeenCalledWith("alice");
    expect(close).toHaveBeenCalled();
    expect(reload).toHaveBeenCalled();
  });

  it("rejects malformed audit data before it reaches the settings UI", async () => {
    const client = createRemoteSecurityClient({
      username: "alice",
      transport: () =>
        ({
          remoteControl: async () => ({
            type: "security",
            security: {
              enabled: true,
              username: "alice",
              route: "alice",
              relay_id: "relay",
              host_pin: "pin",
              authority: null,
              retained_history: null,
              live_circuits: "not-an-array",
            },
          }),
        }) as unknown as RemoteApplicationWebSocket,
      application: { close: async () => undefined } as ApplicationViewClient,
      store: store(),
    });
    await expect(client.state()).rejects.toThrow("invalid security view");
  });
});

function result(operation: RemoteControlOperation): RemoteControlOutcome {
  switch (operation.type) {
    case "inspect":
      return {
        type: "security",
        security: {
          enabled: true,
          username: "alice",
          route: "alice",
          relay_id: "relay",
          host_pin: "pin",
          authority: null,
          retained_history: null,
          live_circuits: [],
        },
      };
    case "revoke_all_other":
    case "require_password_everywhere":
      return { type: "count", count: 2 };
    case "sign_out_this_browser":
      return { type: "signed_out", authorization_revoked: true };
    default:
      return { type: "complete" };
  }
}

function store(overrides: Partial<RemoteClientStore> = {}): RemoteClientStore {
  return {
    load: async () => undefined,
    saveTrust: async () => undefined,
    saveAuthorization: async () => undefined,
    clearAuthorization: async () => undefined,
    clearHost: async () => undefined,
    ...overrides,
  };
}

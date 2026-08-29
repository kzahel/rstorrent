// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteCryptoWasmModule } from "../remote-application-websocket";
import type {
  RemoteClientStore,
  StoredRemoteClient,
} from "../remote-client-store";
import { RemoteAccessGate } from "./RemoteAccessGate";

const crypto = {} as RemoteCryptoWasmModule;

describe("remote access gate", () => {
  beforeEach(() => {
    vi.stubGlobal("localStorage", memoryStorage());
    vi.stubGlobal("sessionStorage", memoryStorage());
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("shows an explicit private or shared browser choice after resume lookup", async () => {
    renderGate(store());
    expect(
      await screen.findByRole("heading", {
        name: "Your torrents, from this browser",
      }),
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: /Private/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /Shared/ })).not.toBeChecked();
    expect(screen.getByLabelText("Password")).toHaveAttribute(
      "autocomplete",
      "current-password",
    );
  });

  it("requires an explicit trust clear after a changed host identity", async () => {
    const clearHost = vi.fn(async () => undefined);
    localStorage.setItem("rstorrent.remote.last-username.v1", "alice");
    sessionStorage.setItem(
      "rstorrent.remote.terminal-failure.v1",
      "host_identity_changed",
    );
    renderGate(store({ clearHost }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "different authenticated host identity",
    );
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Clear old host trust" }),
    );
    await waitFor(() => expect(clearHost).toHaveBeenCalledWith("alice"));
    expect(await screen.findByLabelText("Password")).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Confirm the host was intentionally reset",
    );
  });
});

function renderGate(clientStore: RemoteClientStore): void {
  render(
    <RemoteAccessGate
      relayUrl="wss://127.0.0.1:7443"
      clientBuild="test-build"
      crypto={crypto}
      store={clientStore}
      onConnected={async () => undefined}
    />,
  );
}

function store(
  overrides: Partial<RemoteClientStore> = {},
): RemoteClientStore {
  return {
    load: async (): Promise<StoredRemoteClient | undefined> => undefined,
    saveTrust: async () => undefined,
    saveAuthorization: async () => undefined,
    clearAuthorization: async () => undefined,
    clearHost: async () => undefined,
    ...overrides,
  };
}

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => {
      values.delete(key);
    },
    setItem: (key, value) => {
      values.set(key, value);
    },
  };
}

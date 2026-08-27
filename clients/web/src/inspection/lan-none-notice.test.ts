// @vitest-environment node

import { describe, expect, it } from "vitest";

import {
  LAN_NONE_NOTICE_STORAGE_KEY,
  NETWORK_NONE_NOTICE_STORAGE_KEY,
  loadCredentialFreeNoticeDismissed,
  saveCredentialFreeNoticeDismissed,
} from "./lan-none-notice";

describe("credential-free LAN notice preference", () => {
  it("accepts only the exact versioned dismissal value", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };

    expect(loadCredentialFreeNoticeDismissed("lan_none", storage)).toBe(false);
    values.set(LAN_NONE_NOTICE_STORAGE_KEY, "yes");
    expect(loadCredentialFreeNoticeDismissed("lan_none", storage)).toBe(false);
    saveCredentialFreeNoticeDismissed("lan_none", storage);
    expect(values.get(LAN_NONE_NOTICE_STORAGE_KEY)).toBe("true");
    expect(loadCredentialFreeNoticeDismissed("lan_none", storage)).toBe(true);

    saveCredentialFreeNoticeDismissed("network_none", storage);
    expect(values.get(NETWORK_NONE_NOTICE_STORAGE_KEY)).toBe("true");
    expect(loadCredentialFreeNoticeDismissed("network_none", storage)).toBe(
      true,
    );
  });

  it("fails open to a visible notice when browser storage is unavailable", () => {
    expect(loadCredentialFreeNoticeDismissed("network_none", null)).toBe(false);
    expect(
      loadCredentialFreeNoticeDismissed("network_none", {
        getItem: () => {
          throw new Error("storage unavailable");
        },
      }),
    ).toBe(false);
    expect(() =>
      saveCredentialFreeNoticeDismissed("network_none", {
        setItem: () => {
          throw new Error("storage unavailable");
        },
      }),
    ).not.toThrow();
  });
});

import { webcrypto } from "node:crypto";
import { describe, expect, it } from "vitest";

import { createPrivateBrowserCredential } from "./remote-client-store";

describe("private browser credential", () => {
  it("creates a non-extractable P-256 signing key and exportable public identity", async () => {
    const credential = await createPrivateBrowserCredential(
      "My browser",
      "test-build",
      webcrypto.subtle as unknown as SubtleCrypto,
      webcrypto as unknown as Crypto,
    );
    expect(credential.key.extractable).toBe(false);
    expect(credential.key.type).toBe("private");
    expect(credential.key.usages).toEqual(["sign"]);
    expect(credential.clientId).toHaveLength(16);
    expect(credential.clientPublicKey).toHaveLength(65);
    expect(credential.clientPublicKey[0]).toBe(4);
  });

  it("rejects labels that cannot be shown safely in owner audit", async () => {
    await expect(
      createPrivateBrowserCredential(
        " browser ",
        undefined,
        webcrypto.subtle as unknown as SubtleCrypto,
        webcrypto as unknown as Crypto,
      ),
    ).rejects.toThrow("trimmed");
  });
});

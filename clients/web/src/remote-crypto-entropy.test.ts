import { describe, expect, it } from "vitest";

import { remoteCryptoOperationEntropy } from "./remote-crypto-entropy";

describe("remoteCryptoOperationEntropy", () => {
  it("requests exactly 32 bytes from the injected secure source", () => {
    let observedLength = 0;
    const entropy = remoteCryptoOperationEntropy({
      getRandomValues(array) {
        if (array === null) {
          throw new Error("unexpected null array");
        }
        observedLength = array.byteLength;
        new Uint8Array(array.buffer, array.byteOffset, array.byteLength).fill(
          0x5a,
        );
        return array;
      },
    });
    expect(observedLength).toBe(32);
    expect([...entropy]).toEqual(new Array(32).fill(0x5a));
  });

  it("fails closed when the platform CSPRNG fails", () => {
    expect(() =>
      remoteCryptoOperationEntropy({
        getRandomValues() {
          throw new Error("platform failure");
        },
      }),
    ).toThrow("secure browser randomness is unavailable");
  });
});

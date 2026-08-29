export type SecureRandomSource = Pick<Crypto, "getRandomValues">;

/**
 * Obtain one operation seed for the shared Wasm remote-crypto core.
 *
 * This adapter deliberately has no pseudorandom or deterministic fallback.
 * Tests inject a source explicitly; production callers use Web Crypto.
 */
export function remoteCryptoOperationEntropy(
  source: SecureRandomSource = globalThis.crypto,
): Uint8Array<ArrayBuffer> {
  if (source?.getRandomValues === undefined) {
    throw new Error("secure browser randomness is unavailable");
  }
  const entropy = new Uint8Array(32);
  try {
    source.getRandomValues(entropy);
  } catch {
    entropy.fill(0);
    throw new Error("secure browser randomness is unavailable");
  }
  return entropy;
}

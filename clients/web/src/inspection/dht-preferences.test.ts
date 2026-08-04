import { describe, expect, it } from "vitest";

import {
  loadDhtVisualizationMode,
  saveDhtVisualizationMode,
} from "./dht-preferences";

class MemoryStorage {
  value: string | null = null;

  getItem(): string | null {
    return this.value;
  }

  setItem(_key: string, value: string): void {
    this.value = value;
  }
}

describe("DHT visualization preferences", () => {
  it("defaults safely and round-trips the literal teaching mode", () => {
    const storage = new MemoryStorage();
    expect(loadDhtVisualizationMode(storage)).toBe("normalized");
    saveDhtVisualizationMode("literal", storage);
    expect(loadDhtVisualizationMode(storage)).toBe("literal");
    storage.value = JSON.stringify({ version: 1, mode: "invented" });
    expect(loadDhtVisualizationMode(storage)).toBe("normalized");
  });
});

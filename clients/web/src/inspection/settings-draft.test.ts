import { describe, expect, it } from "vitest";

import {
  compareRevisions,
  initializeSettingsDraft,
  reduceSettingsDraft,
  settingsDraftPhase,
  settingsDraftValue,
  type SettingsDraftComparators,
  type SettingsDraftState,
} from "./settings-draft";

interface Values {
  readonly upload: number;
  readonly download: number;
  readonly enabled: boolean;
}

const equal: SettingsDraftComparators<Values> = {
  upload: Object.is,
  download: Object.is,
  enabled: Object.is,
};

function authority(
  state: SettingsDraftState<Values>,
  revision: string,
  value: Values,
) {
  return reduceSettingsDraft(
    state,
    { type: "authority", resourceKey: "torrent-a", revision, value },
    equal,
  );
}

function edit(
  state: SettingsDraftState<Values>,
  field: keyof Values,
  value: number | boolean,
) {
  return reduceSettingsDraft(
    state,
    { type: "edit", field, value },
    equal,
  );
}

describe("settings draft reducer", () => {
  it("keeps dirty fields through cloned updates while clean fields follow authority", () => {
    let state = initializeSettingsDraft("torrent-a", "4", {
      upload: 10,
      download: 20,
      enabled: true,
    });
    state = edit(state, "download", 0);
    state = authority(state, "5", {
      upload: 11,
      download: 20,
      enabled: true,
    });
    expect(settingsDraftValue(state)).toEqual({
      upload: 11,
      download: 0,
      enabled: true,
    });
    expect(state.conflicts).toEqual([]);

    state = authority(state, "6", {
      upload: 11,
      download: 21,
      enabled: true,
    });
    expect(settingsDraftValue(state)?.download).toBe(0);
    expect(state.conflicts).toEqual(["download"]);
    expect(settingsDraftPhase(state)).toBe("conflict");
  });

  it("waits for a correlated receipt and sufficiently new matching authority", () => {
    let state = initializeSettingsDraft("torrent-a", "7", {
      upload: 10,
      download: 20,
      enabled: true,
    });
    state = edit(state, "download", 0);
    state = reduceSettingsDraft(state, { type: "submit" }, equal);
    expect(settingsDraftPhase(state)).toBe("submitting");

    state = authority(state, "7", {
      upload: 12,
      download: 20,
      enabled: true,
    });
    state = reduceSettingsDraft(
      state,
      { type: "accept", revision: "8" },
      equal,
    );
    expect(settingsDraftPhase(state)).toBe("awaiting_view");
    expect(settingsDraftValue(state)?.download).toBe(0);

    state = authority(state, "8", {
      upload: 12,
      download: 0,
      enabled: true,
    });
    expect(settingsDraftPhase(state)).toBe("pristine");
    expect(state.overlays).toEqual({});
  });

  it("converges a semantic no-op immediately after its receipt", () => {
    let state = initializeSettingsDraft("torrent-a", "9", {
      upload: 10,
      download: 20,
      enabled: true,
    });
    state = edit(state, "download", 21);
    state = reduceSettingsDraft(state, { type: "submit" }, equal);
    state = authority(state, "9", {
      upload: 10,
      download: 21,
      enabled: true,
    });
    expect(settingsDraftPhase(state)).toBe("submitting");
    state = reduceSettingsDraft(
      state,
      { type: "accept", revision: "9" },
      equal,
    );
    expect(settingsDraftPhase(state)).toBe("pristine");
  });

  it("does not erase a newer edit when the captured submission converges", () => {
    let state = initializeSettingsDraft("torrent-a", "10", {
      upload: 10,
      download: 20,
      enabled: true,
    });
    state = edit(state, "download", 21);
    state = reduceSettingsDraft(state, { type: "submit" }, equal);
    state = edit(state, "download", 22);
    state = reduceSettingsDraft(
      state,
      { type: "accept", revision: "11" },
      equal,
    );
    state = authority(state, "11", {
      upload: 10,
      download: 21,
      enabled: true,
    });
    expect(settingsDraftPhase(state)).toBe("dirty");
    expect(settingsDraftValue(state)?.download).toBe(22);
    expect(state.editBases.download).toBe(21);
  });

  it("preserves overlays on failure and bounds the reported failure", () => {
    let state = initializeSettingsDraft("torrent-a", "12", {
      upload: 10,
      download: 20,
      enabled: true,
    });
    state = edit(state, "enabled", false);
    state = reduceSettingsDraft(state, { type: "submit" }, equal);
    state = reduceSettingsDraft(
      state,
      { type: "fail", message: `  ${"x".repeat(600)}  ` },
      equal,
    );
    expect(settingsDraftPhase(state)).toBe("failed");
    expect(settingsDraftValue(state)?.enabled).toBe(false);
    expect(state.failure).toHaveLength(512);
  });

  it("preserves same-resource overlays across resets and isolates resource keys", () => {
    let state = initializeSettingsDraft("torrent-a", "13", {
      upload: 10,
      download: 20,
      enabled: true,
    });
    state = edit(state, "upload", 99);
    state = authority(state, "14", {
      upload: 10,
      download: 30,
      enabled: true,
    });
    expect(settingsDraftValue(state)).toEqual({
      upload: 99,
      download: 30,
      enabled: true,
    });
    state = reduceSettingsDraft(
      state,
      {
        type: "authority",
        resourceKey: "torrent-b",
        revision: "1",
        value: { upload: 1, download: 2, enabled: false },
      },
      equal,
    );
    expect(settingsDraftValue(state)).toEqual({
      upload: 1,
      download: 2,
      enabled: false,
    });
    expect(state.overlays).toEqual({});
  });

  it("orders arbitrary-size decimal revisions without numeric conversion", () => {
    expect(compareRevisions("9", "10")).toBe(-1);
    expect(compareRevisions("18446744073709551616", "9999999999999999999")).toBe(1);
    expect(() => compareRevisions("01", "1")).toThrow();
  });
});

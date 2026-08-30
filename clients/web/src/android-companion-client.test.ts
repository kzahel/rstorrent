import { describe, expect, it } from "vitest";

import type { ApiBackendIdentity, ApiHello } from "./api/generated/v1";
import {
  sameBackendIdentity,
  validateAndroidBackend,
  withCompanionOrigin,
} from "./android-companion-client";

const backend: ApiBackendIdentity = {
  kind: "android",
  instance_id: "abcdefghijklmnop",
  profile_id: "default",
  product_version: "0.1.0",
  capability_profile: [
    "android_saf_acquisition",
    "retained_storage_roots",
    "one_current_root",
    "joined_platform_root_removal",
  ],
};

function hello(overrides: Partial<ApiHello> = {}): ApiHello {
  return {
    api: { current: 1, minimum: 1 },
    encodings: ["json"],
    deliveries: ["stream"],
    capabilities: ["torrent_list", "torrent_files"],
    backend,
    limits: {
      max_view_sets_per_owner: 8,
      max_views_per_set: 16,
      max_view_id_bytes: 64,
      min_queue_bytes: 1024,
      default_queue_bytes: 4096,
      max_queue_bytes: 65_536,
      max_snapshot_bytes: 1_048_576,
      max_wait_millis: 20_000,
      lease_millis: "30000",
    },
    ...overrides,
  };
}

describe("Android companion identity", () => {
  it("pins every HTTP request to the packaged extension origin", () => {
    const request = withCompanionOrigin(
      { headers: { Accept: "application/json", Origin: "https://wrong.example" } },
      "chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc",
    );
    const headers = new Headers(request.headers);
    expect(headers.get("Accept")).toBe("application/json");
    expect(headers.get("Origin")).toBe(
      "chrome-extension://gcgoepclopkgijmclmlheafaglmbjlcc",
    );
    expect(() =>
      withCompanionOrigin({}, "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ).toThrow(/recognized extension origin/u);
  });

  it("requires the exact Android root profile without media delivery", () => {
    expect(validateAndroidBackend(hello())).toEqual(backend);
    expect(() =>
      validateAndroidBackend(
        hello({ capabilities: ["torrent_list", "torrent_media"] }),
      ),
    ).toThrow(/media/u);
    expect(() =>
      validateAndroidBackend(
        hello({ backend: { ...backend, capability_profile: [] } }),
      ),
    ).toThrow(/incomplete/u);
  });

  it("keys saved authority to backend instance and profile", () => {
    expect(sameBackendIdentity(backend, { ...backend })).toBe(true);
    expect(
      sameBackendIdentity(backend, {
        ...backend,
        instance_id: "qrstuvwxyzabcdef",
      }),
    ).toBe(false);
    expect(sameBackendIdentity(backend, { ...backend, profile_id: "other" })).toBe(false);
  });
});

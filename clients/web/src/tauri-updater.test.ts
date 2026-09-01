import { describe, expect, it } from "vitest";

import { desktopUpdateCheckHeaders } from "./tauri-updater";

describe("desktop updater product context", () => {
  it("omits a stable identifier when native product policy withholds it", () => {
    expect(desktopUpdateCheckHeaders("startup", null)).toEqual({
      "X-Check-Reason": "startup",
    });
  });

  it("uses the closed check-only header allowlist when product policy permits it", () => {
    expect(
      desktopUpdateCheckHeaders(
        "manual",
        "87e66203-9849-44c5-a557-8e77c29e7587",
      ),
    ).toEqual({
      "X-Check-Reason": "manual",
      "X-CFU-Id": "87e66203-9849-44c5-a557-8e77c29e7587",
    });
  });
});

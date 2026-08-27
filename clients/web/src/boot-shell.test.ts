// @vitest-environment node

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

describe("production boot shell", () => {
  it("loads a classic failure guard before the application module", () => {
    const document = readFileSync(new URL("../index.html", import.meta.url), "utf8");
    const guard = '<script src="/rstorrent-boot.js"></script>';
    const application = '<script type="module" src="/src/main.ts"></script>';

    expect(document).toContain(guard);
    expect(document).toContain('id="rstorrent-boot-status"');
    expect(document).toContain("RSTorrent requires JavaScript in this browser.");
    expect(document.indexOf(guard)).toBeLessThan(document.indexOf(application));
  });

  it("keeps the failure guard compatible with browsers that cannot run modules", () => {
    const source = readFileSync(
      new URL("../public/rstorrent-boot.js", import.meta.url),
      "utf8",
    );

    expect(source).toContain('window.addEventListener("error", showFailure)');
    expect(source).toContain("window.setTimeout(showFailure, 10000)");
    expect(source).not.toMatch(/(?:=>|\bconst\b|\blet\b|\?\.|\?\?)/);
  });
});

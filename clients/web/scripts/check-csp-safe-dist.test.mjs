import { describe, expect, it } from "vitest";

import { dynamicCodeViolations } from "./check-csp-safe-dist.mjs";

describe("production CSP check", () => {
  it("rejects the Function constructor form used by runtime compilers", () => {
    expect(
      dynamicCodeViolations(
        'const validate = Function("self", "scope", source)(self, scope);',
      ),
    ).toEqual(["Function constructor"]);
    expect(dynamicCodeViolations('const validate = new Function("value");')).toEqual(
      ["Function constructor"],
    );
  });

  it("rejects direct eval and accepts ordinary functions", () => {
    expect(dynamicCodeViolations("const value = eval(source);")).toEqual([
      "direct eval",
    ]);
    expect(
      dynamicCodeViolations("function validate(value) { return value !== null; }"),
    ).toEqual([]);
  });
});

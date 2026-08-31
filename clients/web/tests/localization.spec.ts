import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const CASES = [
  { locale: "en-XA", direction: "ltr" },
  { locale: "ar-XB", direction: "rtl" },
] as const;

const VIEWPORTS = [
  { width: 1_440, height: 900 },
  { width: 920, height: 720 },
  { width: 390, height: 844 },
] as const;

for (const localeCase of CASES) {
  test(`${localeCase.locale} covers responsive product and settings`, async ({ page }) => {
    for (const viewport of VIEWPORTS) {
      await page.setViewportSize(viewport);
      await page.goto(
        `/?demo=healthy-download&at=42000&autoplay=0&locale=${localeCase.locale}`,
      );
      await expect(page.locator("html")).toHaveAttribute("lang", localeCase.locale);
      await expect(page.locator("html")).toHaveAttribute("dir", localeCase.direction);
      await expect(page.locator("[data-destination]").first()).toBeVisible();
      const bodyText = await page.locator("body").innerText();
      expect(bodyText).not.toMatch(/(?:inspection|common|shell)\.[a-z0-9.-]+/);
      expect(bodyText).toContain("⟦");
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
      ).toBe(true);

      await page.locator('header button[aria-haspopup="dialog"]').click();
      await expect(page.getByRole("dialog")).toBeVisible();
      const violations = (await new AxeBuilder({ page }).analyze()).violations.filter(
        (violation) => violation.impact === "serious" || violation.impact === "critical",
      );
      expect(violations).toEqual([]);
      await page.keyboard.press("Escape");
    }
  });
}

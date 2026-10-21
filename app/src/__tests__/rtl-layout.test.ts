import { describe, it, expect } from "vitest";
import { isRtlLocale, getDirection, RTL_LOCALES, LOCALES, type Locale } from "../lib/i18n";

describe("RTL Layout Support (Issue #621)", () => {
  it("recognizes Arabic as an RTL fixture locale", () => {
    expect(RTL_LOCALES).toContain("ar");
    expect(isRtlLocale("ar")).toBe(true);
    expect(getDirection("ar")).toBe("rtl");
  });

  it("recognizes LTR locales correctly", () => {
    const ltrLocales: Locale[] = ["en", "es", "de", "fr", "ja", "zh"];
    for (const locale of ltrLocales) {
      expect(isRtlLocale(locale)).toBe(false);
      expect(getDirection(locale)).toBe("ltr");
    }
  });

  it("handles all defined LOCALES deterministically", () => {
    for (const locale of LOCALES) {
      const dir = getDirection(locale);
      expect(["rtl", "ltr"]).toContain(dir);
      if (dir === "rtl") {
        expect(RTL_LOCALES).toContain(locale);
      } else {
        expect(RTL_LOCALES).not.toContain(locale);
      }
    }
  });
});

import { describe, expect, it } from "vitest";
import {
  THEMES,
  THEME_CSS_VARIABLES,
  THEME_COLOR_FIELDS,
  applyTheme,
  themeToCssVariables,
  validateTheme,
  validateThemes,
} from "../lib/themes";
import exampleTheme from "../lib/themes/community/neon-noir.json";

describe("theme contract", () => {
  it("defines a CSS variable for every felt/deck field", () => {
    expect(THEME_COLOR_FIELDS).toEqual([
      "feltDark",
      "feltMid",
      "feltLight",
      "feltShadow",
      "uiBorder",
      "cardBackStart",
      "cardBackEnd",
      "cardBackBorder",
      "cardBackSymbol",
    ]);
    for (const field of THEME_COLOR_FIELDS) {
      expect(THEME_CSS_VARIABLES[field]).toMatch(/^--[a-z0-9-]+$/);
    }
  });

  it("ships only valid built-in themes with unique ids", () => {
    const result = validateThemes(THEMES);
    expect(result.errors).toEqual([]);
    expect(result.valid).toBe(true);
  });

  it("accepts the example community theme", () => {
    const result = validateTheme(exampleTheme);
    expect(result.errors).toEqual([]);
    expect(result.valid).toBe(true);

    const variables = themeToCssVariables(exampleTheme);
    expect(variables["--felt-mid"]).toBe(exampleTheme.feltMid);
    expect(variables["--card-back-symbol"]).toBe(exampleTheme.cardBackSymbol);
  });

  it("rejects missing fields, invalid colors, and duplicate ids", () => {
    const { feltShadow, ...missingField } = THEMES[0];
    expect(validateTheme(missingField).errors.join(" ")).toContain("feltShadow");

    expect(
      validateTheme({ ...THEMES[0], id: "Bad Id", feltMid: "red; display:none" })
        .errors.join(" ")
    ).toMatch(/id must be|feltMid must be/);

    const duplicate = validateThemes([THEMES[0], THEMES[0]]);
    expect(duplicate.valid).toBe(false);
    expect(duplicate.errors.join(" ")).toContain("duplicate theme id");
  });

  it("applies the contract variables to the document root", () => {
    applyTheme(THEMES[0]);
    const style = document.documentElement.style;
    expect(style.getPropertyValue("--felt-dark")).toBe(THEMES[0].feltDark);
    expect(style.getPropertyValue("--felt-shadow")).toBe(THEMES[0].feltShadow);
    expect(style.getPropertyValue("--card-back-start")).toBe(
      THEMES[0].cardBackStart
    );
    expect(style.getPropertyValue("--card-back-symbol")).toBe(
      THEMES[0].cardBackSymbol
    );
  });
});

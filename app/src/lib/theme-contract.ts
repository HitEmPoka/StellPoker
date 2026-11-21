import contract from "./theme-contract.json";

/**
 * Keys that every theme must provide. Each key maps to the CSS custom
 * property it overrides at runtime.
 */
export type ThemeColorField = keyof typeof contract.cssVariables;

/** Public theme contract shipped to community theme authors. */
export const THEME_CONTRACT = contract;

/** Theme field -> CSS custom property name. */
export const THEME_CSS_VARIABLES: Record<ThemeColorField, string> =
  contract.cssVariables;

/** Ordered list of every theme color field. */
export const THEME_COLOR_FIELDS = Object.keys(
  contract.cssVariables
) as ThemeColorField[];

/** A table felt and card deck theme. */
export type TableTheme = {
  id: string;
  label: string;
} & Record<ThemeColorField, string>;

/** Result of validating a theme object. */
export interface ThemeValidationResult {
  valid: boolean;
  errors: string[];
}

// Hex, rgb(a), hsl(a), modern color functions, CSS named colors, or a
// reference to another custom property. Values containing CSS structure
// characters are rejected so a community theme cannot break out of a value.
const CSS_COLOR_PATTERN =
  /^(#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})|(?:rgb|rgba|hsl|hsla|oklch|oklab|lab|lch|color)\([^;{}<>]*\)|var\(--[a-z0-9-]+\)|[a-z]+)$/i;

const THEME_ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** Returns true when a value is a safe CSS color token. */
export function isCssColor(value: unknown): value is string {
  return typeof value === "string" && CSS_COLOR_PATTERN.test(value.trim());
}

/**
 * Validates a theme against the public contract. Unknown fields are allowed
 * so themes can carry presentation metadata, but every required field must be
 * present and every color must be a safe CSS color token.
 */
export function validateTheme(value: unknown): ThemeValidationResult {
  const errors: string[] = [];

  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { valid: false, errors: ["theme must be a JSON object"] };
  }

  const theme = value as Record<string, unknown>;

  if (typeof theme.id !== "string" || !THEME_ID_PATTERN.test(theme.id)) {
    errors.push(
      'id must be a lowercase slug (letters, numbers, and single hyphens), e.g. "neon-noir"'
    );
  } else if (theme.id.length > 64) {
    errors.push("id must be 64 characters or fewer");
  }

  if (
    typeof theme.label !== "string" ||
    theme.label.trim().length === 0 ||
    theme.label.length > 64
  ) {
    errors.push("label must be a non-empty string of 64 characters or fewer");
  }

  for (const field of THEME_COLOR_FIELDS) {
    const color = theme[field];
    if (typeof color !== "string" || color.trim().length === 0) {
      errors.push(
        `${field} is required (CSS custom property ${THEME_CSS_VARIABLES[field]})`
      );
      continue;
    }
    if (!isCssColor(color)) {
      errors.push(
        `${field} must be a valid CSS color (hex, rgb/hsl, named, or var()), got "${color}"`
      );
    }
  }

  return { valid: errors.length === 0, errors };
}

/** Validates a collection of themes and rejects duplicate ids. */
export function validateThemes(values: unknown[]): ThemeValidationResult {
  const errors: string[] = [];
  const seen = new Set<string>();

  values.forEach((value, index) => {
    const result = validateTheme(value);
    for (const error of result.errors) {
      errors.push(`themes[${index}]: ${error}`);
    }

    if (
      typeof value === "object" &&
      value !== null &&
      typeof (value as Record<string, unknown>).id === "string"
    ) {
      const id = (value as Record<string, unknown>).id as string;
      if (seen.has(id)) {
        errors.push(`themes[${index}]: duplicate theme id "${id}"`);
      }
      seen.add(id);
    }
  });

  return { valid: errors.length === 0, errors };
}

/** Converts a theme to the CSS custom properties it overrides. */
export function themeToCssVariables(theme: TableTheme): Record<string, string> {
  return THEME_COLOR_FIELDS.reduce<Record<string, string>>((vars, field) => {
    vars[THEME_CSS_VARIABLES[field]] = theme[field];
    return vars;
  }, {});
}

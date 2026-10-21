#!/usr/bin/env node
/**
 * Validates the built-in and community theme definitions against the public
 * theme contract in app/src/lib/theme-contract.json.
 *
 * Uses only Node built-ins so it can run in CI before (or without) installing
 * frontend dependencies.
 */
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appLib = join(repoRoot, "app", "src", "lib");
const contractPath = join(appLib, "theme-contract.json");
const builtInThemesPath = join(appLib, "themes.json");
const communityThemesDir = join(appLib, "themes", "community");

const CSS_COLOR_PATTERN =
  /^(#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})|(?:rgb|rgba|hsl|hsla|oklch|oklab|lab|lch|color)\([^;{}<>]*\)|var\(--[a-z0-9-]+\)|[a-z]+)$/i;
const THEME_ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

const errors = [];
const themes = [];
let colorFields = [];

function readJson(path, label) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    errors.push(`${label}: unable to read valid JSON (${error.message})`);
    return null;
  }
}

function isCssColor(value) {
  return typeof value === "string" && CSS_COLOR_PATTERN.test(value.trim());
}

function validateTheme(theme, label) {
  if (typeof theme !== "object" || theme === null || Array.isArray(theme)) {
    errors.push(`${label}: theme must be a JSON object`);
    return;
  }

  if (typeof theme.id !== "string" || !THEME_ID_PATTERN.test(theme.id)) {
    errors.push(
      `${label}: id must be a lowercase slug (letters, numbers, and single hyphens)`
    );
  } else if (theme.id.length > 64) {
    errors.push(`${label}: id must be 64 characters or fewer`);
  }

  if (
    typeof theme.label !== "string" ||
    theme.label.trim().length === 0 ||
    theme.label.length > 64
  ) {
    errors.push(`${label}: label must be a non-empty string of 64 characters or fewer`);
  }

  for (const field of colorFields) {
    const color = theme[field];
    if (typeof color !== "string" || color.trim().length === 0) {
      errors.push(
        `${label}: ${field} is required (CSS custom property ${cssVariables[field]})`
      );
      continue;
    }
    if (!isCssColor(color)) {
      errors.push(
        `${label}: ${field} must be a valid CSS color, got "${color}"`
      );
    }
  }
}

const contract = readJson(contractPath, "theme-contract.json");
const builtInThemes = readJson(builtInThemesPath, "themes.json");

if (!contract) {
  finish();
}

const cssVariables = contract.cssVariables;
if (typeof cssVariables !== "object" || cssVariables === null) {
  errors.push("theme-contract.json: cssVariables must be an object");
  finish();
}

colorFields = Object.keys(cssVariables);
const requiredFields = Array.isArray(contract.requiredFields)
  ? contract.requiredFields
  : [];
const expectedFields = ["id", "label", ...colorFields];

for (const field of expectedFields) {
  if (!requiredFields.includes(field)) {
    errors.push(`theme-contract.json: requiredFields is missing "${field}"`);
  }
}
for (const field of requiredFields) {
  if (!expectedFields.includes(field)) {
    errors.push(`theme-contract.json: required field "${field}" has no CSS variable`);
  }
}

const seenVariableNames = new Set();
for (const [field, variable] of Object.entries(cssVariables)) {
  if (typeof variable !== "string" || !variable.startsWith("--")) {
    errors.push(
      `theme-contract.json: cssVariables.${field} must be a CSS custom property starting with "--"`
    );
  } else if (seenVariableNames.has(variable)) {
    errors.push(
      `theme-contract.json: cssVariables.${field} duplicates "${variable}"`
    );
  }
  seenVariableNames.add(variable);
}

if (Array.isArray(builtInThemes)) {
  builtInThemes.forEach((theme, index) =>
    themes.push({ theme, label: `themes.json[${index}]` })
  );
} else if (builtInThemes) {
  errors.push("themes.json: expected an array of themes");
}

if (existsSync(communityThemesDir)) {
  const files = readdirSync(communityThemesDir)
    .filter((file) => file.endsWith(".json"))
    .sort();
  for (const file of files) {
    const theme = readJson(join(communityThemesDir, file), `community/${file}`);
    if (theme) themes.push({ theme, label: `community/${file}` });
  }
  if (files.length === 0) {
    errors.push("community themes directory contains no .json themes");
  }
}

const seenIds = new Map();
for (const { theme, label } of themes) {
  validateTheme(theme, label);
  if (theme && typeof theme.id === "string") {
    if (seenIds.has(theme.id)) {
      errors.push(
        `${label}: duplicate theme id "${theme.id}" (also in ${seenIds.get(theme.id)})`
      );
    } else {
      seenIds.set(theme.id, label);
    }
  }
}

function finish() {
  if (errors.length > 0) {
    console.error(`Theme validation failed with ${errors.length} error(s):`);
    for (const error of errors) console.error(`  - ${error}`);
    process.exit(1);
  }

  console.log(
    `Theme validation passed: ${themes.length} theme(s), ${colorFields.length} CSS variables.`
  );
  process.exit(0);
}

finish();

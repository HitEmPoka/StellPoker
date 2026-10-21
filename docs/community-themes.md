# Community Theme API

StellPoker themes are a small, stable set of CSS custom properties. The same
contract is used by the built-in themes, the theme selector, and community
themes, so a theme can be distributed as JSON or as a plain CSS override.

The machine-readable contract lives in
[`app/src/lib/theme-contract.json`](../app/src/lib/theme-contract.json).
A complete example theme lives in
[`app/src/lib/themes/community/neon-noir.json`](../app/src/lib/themes/community/neon-noir.json).

## Contract

Every theme must provide an `id`, a human-readable `label`, and the following
colors:

| JSON key | CSS variable | Purpose |
|---|---|---|
| `feltDark` | `--felt-dark` | Outer edge of the table felt radial gradient |
| `feltMid` | `--felt-mid` | Middle stop of the felt gradient |
| `feltLight` | `--felt-light` | Center/highlight of the felt |
| `feltShadow` | `--felt-shadow` | Deep shadow tone for the table edge |
| `uiBorder` | `--ui-border` | Rail, panel, and modal borders |
| `cardBackStart` | `--card-back-start` | Top of the card-back gradient |
| `cardBackEnd` | `--card-back-end` | Bottom of the card-back gradient |
| `cardBackBorder` | `--card-back-border` | Inner crosshatch border on the card back |
| `cardBackSymbol` | `--card-back-symbol` | Center symbol on the card back |

`id` must be a lowercase slug (letters, numbers, and single hyphens) and
unique among built-in and community themes. `label` must be 1–64 characters.
Color values may be hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`),
`rgb()`, `rgba()`, `hsl()`, `hsla()`, `oklch()`, a CSS named color, or a
reference to another custom property such as `var(--pixel-gold)`.

## Example theme

`app/src/lib/themes/community/neon-noir.json`:

```json
{
  "id": "neon-noir",
  "label": "Neon Noir",
  "feltDark": "#12021f",
  "feltMid": "#260646",
  "feltLight": "#4b0f7a",
  "feltShadow": "#07000d",
  "uiBorder": "#ff2e97",
  "cardBackStart": "#1b0b33",
  "cardBackEnd": "#08040f",
  "cardBackBorder": "#00e5ff",
  "cardBackSymbol": "#ff2e97"
}
```

Paste this theme into the table's theme list to make it selectable, or apply it
as CSS without touching JavaScript:

```css
/* Load after globals.css. */
:root {
  --felt-dark: #12021f;
  --felt-mid: #260646;
  --felt-light: #4b0f7a;
  --felt-shadow: #07000d;
  --ui-border: #ff2e97;
  --card-back-start: #1b0b33;
  --card-back-end: #08040f;
  --card-back-border: #00e5ff;
  --card-back-symbol: #ff2e97;
}
```

The theme API is also available from TypeScript:

```ts
import {
  THEME_CSS_VARIABLES,
  themeToCssVariables,
  validateTheme,
  type TableTheme,
} from "@/lib/themes";

const result = validateTheme(myTheme);
if (result.valid) {
  const variables = themeToCssVariables(myTheme as TableTheme);
}
```

## Validation

Theme data is validated in CI by `scripts/validate-themes.mjs`. The script
checks the built-in themes in `app/src/lib/themes.json` and every JSON file in
`app/src/lib/themes/community/` for:

- every required contract field;
- valid, safe CSS color values;
- valid, unique theme ids;
- a matching CSS variable for every contract field.

Run it locally with:

```bash
node scripts/validate-themes.mjs
```

The same rules are covered by `app/src/__tests__/themes.test.ts`:

```bash
cd app
npm run test:themes
```

## Theme guidelines

- Keep the felt center (`feltLight`) lighter than the edge (`feltDark`) so the
  radial gradient still reads as a table.
- Keep `cardBackSymbol` high-contrast against `cardBackStart`/`cardBackEnd`.
- `uiBorder` should remain visible against both the felt and the page
  background.
- Avoid pure black card faces; card faces stay light for readability and are
  intentionally not part of this contract.
- Test the theme in both light and dark UI modes and at mobile widths.

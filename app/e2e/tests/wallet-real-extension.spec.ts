import { test, expect, chromium, type BrowserContext } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/**
 * Optional real-extension smoke test (issue #651).
 *
 * The always-on suite (`src/__tests__/wallet-integration.test.ts`) uses the
 * Freighter test double and runs on every pull request. This spec is the second
 * tier: it loads the *real* official Freighter extension into Chromium, proves
 * the extension injects its API, and (optionally) completes a connection from a
 * pre-approved profile.
 *
 * It is skipped unless `REAL_FREIGHTER_SMOKE=1` so normal CI never depends on
 * a browser extension or a funded wallet. See
 * `docs/wallet-integration-testing.md` for the full runbook.
 */

const ENABLED = process.env.REAL_FREIGHTER_SMOKE === "1";
const EXTENSION_PATH = process.env.FREIGHTER_EXTENSION_PATH;
const CONNECT = process.env.REAL_FREIGHTER_CONNECT === "1";
const HEADLESS = process.env.REAL_FREIGHTER_HEADLESS === "1";

test.describe("Freighter real extension smoke", () => {
  test.skip(
    !ENABLED,
    "Set REAL_FREIGHTER_SMOKE=1 and FREIGHTER_EXTENSION_PATH to run the real-extension smoke."
  );
  test.skip(
    !EXTENSION_PATH,
    "Set FREIGHTER_EXTENSION_PATH to an unpacked official Freighter extension."
  );

  test("loads Freighter, injects its API, and is detected by the app", async () => {
    test.setTimeout(60_000);
    const extensionPath = EXTENSION_PATH!;
    if (!fs.existsSync(extensionPath)) {
      throw new Error(
        `FREIGHTER_EXTENSION_PATH does not exist: ${extensionPath}. ` +
          "Run scripts/fetch-freighter-extension.py or point it at an unpacked extension."
      );
    }

    const providedProfile = process.env.FREIGHTER_USER_DATA_DIR;
    const userDataDir =
      providedProfile ??
      fs.mkdtempSync(path.join(os.tmpdir(), "stellpoker-freighter-"));

    let context: BrowserContext | undefined;
    try {
      context = await chromium.launchPersistentContext(userDataDir, {
        channel: "chromium",
        headless: HEADLESS,
        args: [
          `--disable-extensions-except=${extensionPath}`,
          `--load-extension=${extensionPath}`,
          "--no-first-run",
        ],
      });

      // A loaded MV3 extension exposes a service worker. It is not required for
      // the smoke (content-script injection is), so treat a missing worker as
      // a diagnostic rather than a failure.
      const serviceWorker =
        context.serviceWorkers()[0] ??
        (await context
          .waitForEvent("serviceworker", { timeout: 5_000 })
          .catch(() => null));
      if (serviceWorker) {
        const extensionId = new URL(serviceWorker.url()).host;
        console.log(`Loaded Freighter extension ${extensionId}`);
      } else {
        console.warn(
          "Freighter service worker did not appear; relying on content-script injection."
        );
      }

      const page = await context.newPage();
      const baseURL = process.env.E2E_BASE_URL || "http://localhost:3000";
      await page.goto(baseURL);
      await page.getByText("CLICK ANYWHERE TO START").click();

      const injected = await page.evaluate(() => {
        const win = window as typeof window & {
          freighterApi?: unknown;
          freighter?: unknown;
          stellar?: { freighterApi?: unknown };
        };
        return Boolean(
          win.freighterApi || win.freighter || win.stellar?.freighterApi
        );
      });
      expect(
        injected,
        "the official Freighter extension should inject a window API"
      ).toBe(true);

      // `isFreighterInstalled()` recognises the legacy injection points, so the
      // lobby must offer the connection even before the user approves access.
      await expect(page.getByText("CONNECT FREIGHTER")).toBeVisible();

      if (CONNECT) {
        // Requires a profile where the site is already approved and the wallet
        // is unlocked. The manual runbook covers preparing that profile.
        await page.getByText("CONNECT FREIGHTER").click();
        await expect(page.getByText(/Freighter:/)).toBeVisible({
          timeout: 20_000,
        });
      }
    } finally {
      await context?.close();
      if (!providedProfile) {
        fs.rmSync(userDataDir, { recursive: true, force: true });
      }
    }
  });
});

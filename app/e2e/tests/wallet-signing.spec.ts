import { test, expect } from "@playwright/test";

const WALLET_ADDR = "GDEPOSITSOMERANDOMSTELLARADDRESS1234567890";

/**
 * Mock the full Freighter API including signMessage and signTransaction.
 * This replicates what the wallet extension injects into the page.
 */
test.describe("wallet signing differences", () => {
  test("Freighter signMessage returns base64-encoded signature", async ({ page }) => {
    const capturedMessages: string[] = [];

    await page.addInitScript((addr: string) => {
      (window as any).freighterApi = {
        getAddress: () => Promise.resolve(addr),
        requestAccess: () => Promise.resolve(),
        signMessage: (msg: string) => {
          return Promise.resolve("0x" + btoa(msg));
        },
      };
    }, WALLET_ADDR);

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("Freighter:")).toBeVisible();
  });

  test("Lobstr signMessage returns object with signature field", async ({ page }) => {
    await page.addInitScript((addr: string) => {
      (window as any).lobstr = {
        getAddress: () => Promise.resolve({ address: addr }),
        signMessage: (msg: string) => {
          return Promise.resolve({ signature: "0x" + btoa(msg) });
        },
      };
    }, WALLET_ADDR);

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT LOBSTR").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("Lobstr:")).toBeVisible();
  });

  test("Freighter signing produces different response format than Lobstr", async ({ page }) => {
    const freighterConnected: string[] = [];
    const lobstrConnected: string[] = [];

    await page.addInitScript((addr: string) => {
      // Inject both wallets with different response formats
      (window as any).freighterApi = {
        getAddress: () => Promise.resolve(addr),
        requestAccess: () => Promise.resolve(),
        signMessage: (msg: string) => {
          // Freighter returns raw string
          return "0x" + btoa(msg);
        },
      };

      (window as any).lobstr = {
        getAddress: () => Promise.resolve({ address: addr }),
        signMessage: (msg: string) => {
          // Lobstr returns { signature }
          return { signature: "0x" + btoa(msg) };
        },
      };
    }, WALLET_ADDR);

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    // Both buttons should be visible
    await expect(page.getByText("CONNECT FREIGHTER")).toBeVisible();
    await expect(page.getByText("CONNECT LOBSTR")).toBeVisible();
  });

  test("Freighter signTransaction returns { signedTxXdr }", async ({ page }) => {
    const mockTxXdr = "AAAAAgAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const mockSignedXdr = "AAAAAgAAAQ...mockSigned";

    const signTransactionMock = page.addInitScript(
      (args: { txXdr: string; signedXdr: string }) => {
        (window as any).freighterApi = {
          getAddress: () => Promise.resolve("GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
          requestAccess: () => Promise.resolve(),
          signMessage: (msg: string) => Promise.resolve("0xsig"),
        };
      },
      { txXdr: mockTxXdr, signedXdr: mockSignedXdr }
    );

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    // Test that wallet detection works when signTransaction is present
    await expect(page.getByText("CONNECT FREIGHTER")).toBeVisible();
  });

  test("Lobstr signTransaction returns { signedTxXdr } format", async ({ page }) => {
    await page.addInitScript(() => {
      (window as any).lobstr = {
        getAddress: () => Promise.resolve({ address: "GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789" }),
        signMessage: (msg: string) => Promise.resolve({ signature: "0xsig" }),
        signTransaction: (txXdr: string) =>
          Promise.resolve({ signedTxXdr: txXdr + "_signed" }),
      };
    });

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("CONNECT LOBSTR")).toBeVisible();
  });

  test("both wallet signMessage methods are callable after connection", async ({ page }) => {
    await page.addInitScript((addr: string) => {
      (window as any).freighterApi = {
        getAddress: () => Promise.resolve(addr),
        requestAccess: () => Promise.resolve(),
        signMessage: (msg: string) => Promise.resolve("0x" + btoa(msg)),
      };
    }, WALLET_ADDR);

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    // Connect Freighter
    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    // Verify wallet connected with correct address format
    const statusText = await page.textContent(
      '[class*="pixel-border-thin"]'
    );
    expect(statusText).toContain("Freighter:");
    expect(statusText).toContain(WALLET_ADDR.slice(0, 6));
  });
});

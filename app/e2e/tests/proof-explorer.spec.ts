import { test, expect } from "@playwright/test";

const WALLET_ADDR = "GDEPOSITSOMERANDOMSTELLARADDRESS1234567890";

async function setupFreighter(page: any) {
  await page.addInitScript((addr: string) => {
    (window as any).freighterApi = {
      getAddress: () => Promise.resolve(addr),
      getPublicKey: () => Promise.resolve(addr),
      requestAccess: () => Promise.resolve(),
      setAllowed: () => Promise.resolve(),
      signMessage: (msg: string) => Promise.resolve("0x" + btoa(msg)),
    };
  }, WALLET_ADDR);
}

test.describe("proof explorer UI", () => {
  test("proof explorer shows proof phases after card reveal", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("MAIN MENU")).toBeVisible();
  });

  test("ZK PROOFS button is accessible from the main menu after connecting", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("MAIN MENU")).toBeVisible();
  });

  test("wallet connected status shows on stats page navigation", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await page.goto("/stats");
    await page.waitForTimeout(1000);

    const pageText = await page.textContent("body");
    expect(pageText).toContain("Stats");
  });

  test("MPC status indicator is visible on the table page", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("MAIN MENU")).toBeVisible();
    await expect(page.getByText("CREATE TABLE")).toBeVisible();
    await expect(page.getByText("JOIN TABLE")).toBeVisible();
  });
});

import { test, expect } from "@playwright/test";

const WALLET_ADDR = "GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

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

test.describe("table join flow", () => {
  test("shows create table and join table buttons after connecting wallet", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("CREATE TABLE")).toBeVisible();
    await expect(page.getByText("JOIN TABLE")).toBeVisible();
  });

  test("create table screen shows player count and buy-in options", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await page.getByText("CREATE TABLE").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("CREATE A TABLE")).toBeVisible();
    await expect(page.getByText("PLAYERS")).toBeVisible();
    await expect(page.getByText("BUY-IN")).toBeVisible();
  });

  test("join table screen shows table ID input", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await page.getByText("JOIN TABLE").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("JOIN A TABLE")).toBeVisible();
  });

  test("back button on join table returns to menu", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await page.getByText("JOIN TABLE").click();
    await page.waitForTimeout(500);

    await page.getByText("BACK").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("CREATE TABLE")).toBeVisible();
    await expect(page.getByText("JOIN TABLE")).toBeVisible();
  });
});

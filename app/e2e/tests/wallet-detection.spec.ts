import { test, expect } from "@playwright/test";

test.describe("wallet detection", () => {
  test("shows connect screen with Freighter detected", async ({ page }) => {
    await page.addInitScript(() => {
      (window as any).freighterApi = {
        getAddress: () => Promise.resolve("GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
        signMessage: () => Promise.resolve("0xsig"),
      };
    });

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("CONNECT FREIGHTER")).toBeVisible();
  });

  test("shows connect screen with Lobstr detected", async ({ page }) => {
    await page.addInitScript(() => {
      (window as any).lobstr = {
        getAddress: () => Promise.resolve({ address: "GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789" }),
        signMessage: () => Promise.resolve({ signature: "0xsig" }),
      };
    });

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("CONNECT LOBSTR")).toBeVisible();
  });

  test("shows both wallet buttons when both installed", async ({ page }) => {
    await page.addInitScript(() => {
      (window as any).freighterApi = {
        getAddress: () => Promise.resolve("GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
        signMessage: () => Promise.resolve("0xsig"),
      };
      (window as any).lobstr = {
        getAddress: () => Promise.resolve({ address: "GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789" }),
        signMessage: () => Promise.resolve({ signature: "0xsig" }),
      };
    });

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("CONNECT FREIGHTER")).toBeVisible();
    await expect(page.getByText("CONNECT LOBSTR")).toBeVisible();
  });

  test("shows NOT DETECTED when no wallet is installed", async ({ page }) => {
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("NOT DETECTED")).toHaveCount(2);
  });
});

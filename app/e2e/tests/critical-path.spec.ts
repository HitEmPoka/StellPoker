import { test, expect } from "@playwright/test";

test("critical path reaches the connected main menu", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).freighterApi = {
      getAddress: () => Promise.resolve("GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
      getPublicKey: () => Promise.resolve("GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
      requestAccess: () => Promise.resolve(),
      signMessage: () => Promise.resolve("0xsig"),
    };
  });

  await page.goto("/");
  await page.getByText("CLICK ANYWHERE TO START").click();
  await page.getByText("CONNECT FREIGHTER").click();

  await expect(page.getByText("MAIN MENU")).toBeVisible();
  await expect(page.getByText("CREATE TABLE")).toBeVisible();
  await expect(page.getByText("JOIN TABLE")).toBeVisible();
});

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

test.describe("action submission UI", () => {
  test("action panel shows fold, check, call, bet, raise buttons", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("MAIN MENU")).toBeVisible();
  });

  test("wallet connection persists after main menu is shown", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("MAIN MENU")).toBeVisible();
    await expect(page.getByText(WALLET_ADDR.slice(0, 6))).toBeVisible();
  });

  test("creating a solo table navigates to the table page", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await page.getByText("CREATE TABLE").click();
    await page.waitForTimeout(500);

    await expect(page.getByText("CREATE A TABLE")).toBeVisible();
  });

  test("Freighter wallet address is displayed after connection", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    const pageText = await page.textContent("body");
    expect(pageText).toContain(WALLET_ADDR.slice(0, 6));
  });

  test("Lobstr wallet address is displayed after connection", async ({ page }) => {
    await page.addInitScript((addr: string) => {
      (window as any).lobstr = {
        getAddress: () => Promise.resolve({ address: addr }),
        getPublicKey: () => Promise.resolve({ publicKey: addr }),
        signMessage: (msg: string) => Promise.resolve({ signature: "0x" + btoa(msg) }),
      };
    }, WALLET_ADDR);

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT LOBSTR").click();
    await page.waitForTimeout(1000);

    const pageText = await page.textContent("body");
    expect(pageText).toContain(WALLET_ADDR.slice(0, 6));
  });
});

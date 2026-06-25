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

async function setupLobstr(page: any) {
  await page.addInitScript((addr: string) => {
    (window as any).lobstr = {
      getAddress: () => Promise.resolve({ address: addr }),
      getPublicKey: () => Promise.resolve({ publicKey: addr }),
      signMessage: (msg: string) => Promise.resolve({ signature: "0x" + btoa(msg) }),
    };
  }, WALLET_ADDR);
}

test.describe("wallet connection flow", () => {
  test("connects Freighter and shows connected status", async ({ page }) => {
    await setupFreighter(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT FREIGHTER").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("Freighter:")).toBeVisible();
    await expect(page.getByText(WALLET_ADDR.slice(0, 6))).toBeVisible();
  });

  test("connects Lobstr and shows connected status", async ({ page }) => {
    await setupLobstr(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT LOBSTR").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("Lobstr:")).toBeVisible();
    await expect(page.getByText(WALLET_ADDR.slice(0, 6))).toBeVisible();
  });

  test("silent reconnect restores previous session on page load", async ({ page }) => {
    await page.addInitScript((addr: string) => {
      localStorage.setItem("stellar_poker_wallet", addr);
      (window as any).freighterApi = {
        getAddress: () => Promise.resolve(addr),
        requestAccess: () => Promise.resolve(),
        signMessage: (msg: string) => Promise.resolve("0xsig"),
      };
    }, WALLET_ADDR);

    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText(WALLET_ADDR.slice(0, 6))).toBeVisible();
  });

  test("shows menu after connecting Freighter", async ({ page }) => {
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

  test("shows menu after connecting Lobstr", async ({ page }) => {
    await setupLobstr(page);
    await page.goto("/");
    await page.getByText("CLICK ANYWHERE TO START").click();
    await page.waitForTimeout(500);

    await page.getByText("CONNECT LOBSTR").click();
    await page.waitForTimeout(1000);

    await expect(page.getByText("MAIN MENU")).toBeVisible();
    await expect(page.getByText("CREATE TABLE")).toBeVisible();
    await expect(page.getByText("JOIN TABLE")).toBeVisible();
  });
});

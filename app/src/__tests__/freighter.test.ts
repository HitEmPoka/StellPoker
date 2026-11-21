import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { FreighterMock } from "./mocks/freighter-mock";
import {
  connectFreighterWallet,
  getSavedWalletAddress,
  isFreighterInstalled,
  trySilentReconnect,
  clearSavedWallet,
} from "@/lib/freighter";

const win = window as unknown as Record<string, unknown>;
let mock: FreighterMock;

beforeEach(() => {
  vi.restoreAllMocks();
  mock = new FreighterMock();
});

afterEach(() => {
  mock.uninstall();
});

describe("freighter.isFreighterInstalled", () => {
  it("returns true when the Freighter mock API is installed", () => {
    mock.install();
    expect(isFreighterInstalled()).toBe(true);
  });

  it("returns false when no Freighter API is present", () => {
    expect(isFreighterInstalled()).toBe(false);
  });
});

describe("freighter.connectFreighterWallet", () => {
  it("returns a session with walletType=freighter using the mock", async () => {
    mock.install();
    const session = await connectFreighterWallet();
    expect(session.address).toBe("GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ABCDEFG");
    expect(session.walletType).toBe("freighter");
    expect(typeof session.signMessage).toBe("function");
  });

  it("throws when Freighter is not installed", async () => {
    await expect(connectFreighterWallet()).rejects.toThrow(/Freighter wallet not found/);
  });

  it("surfaces a user sign rejection", async () => {
    mock.install();
    const session = await connectFreighterWallet();
    mock.simulateUserRejection();
    await expect(session.signMessage("hello")).rejects.toThrow(/User rejected request/);
  });
});

describe("freighter.getSavedWalletAddress / clearSavedWallet", () => {
  it("round-trips the saved address in localStorage", () => {
    expect(getSavedWalletAddress()).toBeFalsy();
    clearSavedWallet();
    expect(getSavedWalletAddress()).toBeFalsy();
  });
});

describe("freighter.trySilentReconnect", () => {
  it("returns null when no saved wallet address", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockReturnValue(null);
    expect(await trySilentReconnect()).toBeNull();
  });
});

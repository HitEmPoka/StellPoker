import { describe, expect, it, vi, beforeEach } from "vitest";
import {
  connectLobstrWallet,
  isLobstrInstalled,
  trySilentReconnectLobstr,
} from "@/lib/lobstr";

const mockAddress = "GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ABCDEFG";
const win = window as unknown as Record<string, unknown>;

beforeEach(() => {
  vi.restoreAllMocks();
  delete win.lobstr;
  vi.spyOn(Storage.prototype, "getItem").mockReturnValue(null);
});

describe("lobstr.isLobstrInstalled", () => {
  it("returns true when window.lobstr exposes getAddress", () => {
    win.lobstr = { getAddress: vi.fn() };
    expect(isLobstrInstalled()).toBe(true);
  });

  it("returns true when window.lobstr exposes signMessage", () => {
    win.lobstr = { signMessage: vi.fn() };
    expect(isLobstrInstalled()).toBe(true);
  });

  it("returns false when no wallet is installed", () => {
    expect(isLobstrInstalled()).toBe(false);
  });
});

describe("lobstr.connectLobstrWallet", () => {
  it("returns a session with walletType=lobstr", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue({ signature: "0xsig" }),
    };
    const session = await connectLobstrWallet();
    expect(session.address).toBe(mockAddress);
    expect(session.walletType).toBe("lobstr");
    expect(typeof session.signMessage).toBe("function");
  });

  it("throws when Lobstr is not installed", async () => {
    await expect(connectLobstrWallet()).rejects.toThrow(/Lobstr wallet not found/);
  });

  it("throws when getAddress API is unavailable", async () => {
    win.lobstr = { signMessage: vi.fn() };
    await expect(connectLobstrWallet()).rejects.toThrow(/getAddress API is unavailable/);
  });

  it("throws when signMessage API is unavailable", async () => {
    win.lobstr = { getAddress: vi.fn().mockResolvedValue({ address: mockAddress }) };
    await expect(connectLobstrWallet()).rejects.toThrow(/signMessage API is unavailable/);
  });

  it("persists the address to localStorage", async () => {
    const setItem = vi.spyOn(window.localStorage, "setItem");
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue("sig"),
    };
    await connectLobstrWallet();
    expect(setItem).toHaveBeenCalledWith("stellar_poker_wallet_lobstr", mockAddress);
  });
});

describe("lobstr.signMessage formats", () => {
  let session: Awaited<ReturnType<typeof connectLobstrWallet>>;

  beforeEach(async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn(),
    };
    session = await connectLobstrWallet();
  });

  it("returns a raw string signature", async () => {
    win.lobstr.signMessage = vi.fn().mockResolvedValue("raw-signature-string");
    expect(await session.signMessage("hello")).toBe("raw-signature-string");
  });

  it("returns signature from { signature } object", async () => {
    win.lobstr.signMessage = vi.fn().mockResolvedValue({ signature: "0xsig" });
    expect(await session.signMessage("hello")).toBe("0xsig");
  });

  it("returns signature from { signedMessage } object", async () => {
    win.lobstr.signMessage = vi.fn().mockResolvedValue({ signedMessage: "sig" });
    expect(await session.signMessage("hello")).toBe("sig");
  });

  it("returns signature from { signed_message } object", async () => {
    win.lobstr.signMessage = vi.fn().mockResolvedValue({ signed_message: "sig" });
    expect(await session.signMessage("hello")).toBe("sig");
  });

  it("throws on error response", async () => {
    win.lobstr.signMessage = vi.fn().mockResolvedValue({ error: "User rejected request" });
    await expect(session.signMessage("test")).rejects.toThrow("User rejected request");
  });

  it("throws on invalid empty object", async () => {
    win.lobstr.signMessage = vi.fn().mockResolvedValue({});
    await expect(session.signMessage("test")).rejects.toThrow(
      /Lobstr returned an invalid signature response/,
    );
  });
});

describe("lobstr.trySilentReconnectLobstr", () => {
  it("returns null when no saved wallet address", async () => {
    expect(await trySilentReconnectLobstr()).toBeNull();
  });

  it("clears saved wallet when API is unavailable", async () => {
    const removeItem = vi.spyOn(window.localStorage, "removeItem");
    vi.spyOn(window.localStorage, "getItem").mockReturnValue(mockAddress);
    expect(await trySilentReconnectLobstr()).toBeNull();
    expect(removeItem).toHaveBeenCalledWith("stellar_poker_wallet_lobstr");
  });
});

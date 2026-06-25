import { describe, it, expect, vi, beforeEach } from "vitest";

const mockAddress = "GABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

const win = window as unknown as Record<string, unknown>;

// ── Wallet Detection Tests ───────────────────────────────────────────────────

describe("wallet detection", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    delete win.lobstr;
    delete win.freighterApi;
    delete win.freighter;
    delete win.stellar;
  });

  it("detects Freighter via legacy window.freighterApi", async () => {
    win.freighterApi = {
      getAddress: vi.fn(),
      signMessage: vi.fn(),
    };

    const { isFreighterInstalled } = await import("../lib/freighter");
    expect(isFreighterInstalled()).toBe(true);
  });

  it("detects Freighter via window.stellar.freighterApi", async () => {
    win.stellar = {
      freighterApi: {
        getAddress: vi.fn(),
        signMessage: vi.fn(),
      },
    };

    const { isFreighterInstalled } = await import("../lib/freighter");
    expect(isFreighterInstalled()).toBe(true);
  });

  it("detects Freighter via window.freighter", async () => {
    win.freighter = {
      getAddress: vi.fn(),
      signMessage: vi.fn(),
    };

    const { isFreighterInstalled } = await import("../lib/freighter");
    expect(isFreighterInstalled()).toBe(true);
  });

  it("detects Lobstr via window.lobstr", async () => {
    win.lobstr = {
      getAddress: vi.fn(),
      signMessage: vi.fn(),
    };

    const { isLobstrInstalled } = await import("../lib/lobstr");
    expect(isLobstrInstalled()).toBe(true);
  });

  it("returns false for both when no wallet is installed", async () => {
    const { isFreighterInstalled } = await import("../lib/freighter");
    const { isLobstrInstalled } = await import("../lib/lobstr");
    expect(isFreighterInstalled()).toBe(false);
    expect(isLobstrInstalled()).toBe(false);
  });

  it("detectInstalledWallets returns both when both installed", async () => {
    win.freighterApi = {
      getAddress: vi.fn(),
      signMessage: vi.fn(),
    };
    win.lobstr = {
      getAddress: vi.fn(),
      signMessage: vi.fn(),
    };

    const { detectInstalledWallets } = await import("../lib/wallet");
    const wallets = detectInstalledWallets();

    expect(wallets.find((w) => w.type === "freighter")?.isInstalled).toBe(true);
    expect(wallets.find((w) => w.type === "lobstr")?.isInstalled).toBe(true);
  });

  it("detectInstalledWallets returns not-installed entries when no wallets present", async () => {
    const { detectInstalledWallets } = await import("../lib/wallet");
    const wallets = detectInstalledWallets();

    const freighterEntry = wallets.find((w) => w.type === "freighter");
    const lobstrEntry = wallets.find((w) => w.type === "lobstr");

    expect(freighterEntry).toBeDefined();
    expect(freighterEntry!.isInstalled).toBe(false);
    expect(lobstrEntry).toBeDefined();
    expect(lobstrEntry!.isInstalled).toBe(false);
  });
});

// ── Wallet Connection Flow Tests ─────────────────────────────────────────────

describe("wallet connection flow", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    delete win.lobstr;
    delete win.freighterApi;
  });

  it("connectLobstrWallet returns session with walletType='lobstr'", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue({ signature: "0xsig" }),
    };

    const { connectLobstrWallet } = await import("../lib/lobstr");
    const session = await connectLobstrWallet();

    expect(session.address).toBe(mockAddress);
    expect(session.walletType).toBe("lobstr");
    expect(typeof session.signMessage).toBe("function");
  });

  it("throws when Lobstr not installed", async () => {
    const { connectLobstrWallet } = await import("../lib/lobstr");
    await expect(connectLobstrWallet()).rejects.toThrow(/Lobstr wallet not found/);
  });

  it("silent reconnect returns null when no saved wallet", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockReturnValue(null);

    const { trySilentReconnect } = await import("../lib/wallet");
    const session = await trySilentReconnect();

    expect(session).toBeNull();
  });

  it("getWalletDisplayName returns correct names", async () => {
    const { getWalletDisplayName } = await import("../lib/wallet");

    expect(
      getWalletDisplayName({ address: "", walletType: "freighter", signMessage: vi.fn() })
    ).toBe("Freighter");
    expect(
      getWalletDisplayName({ address: "", walletType: "lobstr", signMessage: vi.fn() })
    ).toBe("Lobstr");
  });
});

// ── Sign message format differences ──────────────────────────────────────────

describe("sign message format differences", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    delete win.lobstr;
    delete win.freighterApi;
  });

  it("Lobstr signMessage accepts { signature } object format", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue({ signature: "0x" + btoa("hello") }),
    };

    const { connectLobstrWallet } = await import("../lib/lobstr");
    const session = await connectLobstrWallet();
    const sig = await session.signMessage("hello");

    expect(sig).toBe("0x" + btoa("hello"));
  });

  it("Lobstr signMessage accepts string format", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue("raw-signature-string"),
    };

    const { connectLobstrWallet } = await import("../lib/lobstr");
    const session = await connectLobstrWallet();
    const sig = await session.signMessage("hello");

    expect(sig).toBe("raw-signature-string");
  });

  it("Lobstr signMessage accepts { signedMessage } object format", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue({ signedMessage: "sig-signed-message" }),
    };

    const { connectLobstrWallet } = await import("../lib/lobstr");
    const session = await connectLobstrWallet();
    const sig = await session.signMessage("hello");

    expect(sig).toBe("sig-signed-message");
  });

  it("Lobstr throws on error response", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue({ error: "User rejected request" }),
    };

    const { connectLobstrWallet } = await import("../lib/lobstr");
    const session = await connectLobstrWallet();

    await expect(session.signMessage("test")).rejects.toThrow("User rejected request");
  });

  it("Lobstr throws on invalid response", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue({}),
    };

    const { connectLobstrWallet } = await import("../lib/lobstr");
    const session = await connectLobstrWallet();

    await expect(session.signMessage("test")).rejects.toThrow(
      /Lobstr returned an invalid signature response/
    );
  });

  it("Lobstr throws on empty object", async () => {
    win.lobstr = {
      getAddress: vi.fn().mockResolvedValue({ address: mockAddress }),
      signMessage: vi.fn().mockResolvedValue({}),
    };

    const { connectLobstrWallet } = await import("../lib/lobstr");
    const session = await connectLobstrWallet();

    await expect(session.signMessage("test")).rejects.toThrow(
      /Lobstr returned an invalid signature response/
    );
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn(), prefetch: vi.fn(), back: vi.fn() }),
  usePathname: () => "/table/7",
  useSearchParams: () => new URLSearchParams(),
}));

vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  getParsedTableState: vi.fn(() => new Promise(() => {})),
  getTableLobby: vi.fn(() => new Promise(() => {})),
  subscribeGameState: vi.fn(() => null),
  getPlayerHudStats: vi.fn(() => new Promise(() => {})),
}));

vi.mock("@/lib/wallet", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/wallet")>()),
  trySilentReconnect: vi.fn(() => Promise.resolve(null)),
  getActiveAddress: vi.fn(() => null),
}));

vi.mock("@/lib/events", () => ({
  subscribePokerTableEvents: vi.fn(() => Promise.resolve({ stop: vi.fn() })),
}));

vi.mock("@/lib/use-wallet-monitor", () => ({
  useWalletMonitor: vi.fn(),
}));

import { Table } from "@/components/Table";
import * as api from "@/lib/api";
import { trySilentReconnect } from "@/lib/wallet";
import { subscribePokerTableEvents } from "@/lib/events";

describe("Table", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("offline"))));
  });

  it("renders the table header for the given table id", () => {
    render(<Table tableId={7} />);
    expect(screen.getByText("TABLE #7")).toBeTruthy();
  });

  it("prompts to connect a wallet when no wallet is seated", () => {
    render(<Table tableId={7} />);
    expect(screen.getAllByText("CONNECT WALLET").length).toBeGreaterThan(0);
  });

  it("syncs on-chain state and subscribes to table updates on mount", async () => {
    render(<Table tableId={7} />);
    await waitFor(() => expect(api.getParsedTableState).toHaveBeenCalledWith(7));
    expect(api.subscribeGameState).toHaveBeenCalledWith(7, expect.any(Function));
    expect(subscribePokerTableEvents).toHaveBeenCalled();
    expect(trySilentReconnect).toHaveBeenCalled();
  });
});

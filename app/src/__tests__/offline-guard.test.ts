import { afterEach, describe, expect, it, vi } from "vitest";
import { getTableState, playerAction } from "@/lib/api";
import { OfflineError, assertOnline, isBrowserOnline } from "@/lib/online";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("offline guard", () => {
  it("treats the browser as online unless it reports offline", () => {
    vi.stubGlobal("navigator", { onLine: true });
    expect(isBrowserOnline()).toBe(true);
    expect(() => assertOnline()).not.toThrow();
  });

  it("throws OfflineError when the browser reports offline", () => {
    vi.stubGlobal("navigator", { onLine: false });
    expect(isBrowserOnline()).toBe(false);
    expect(() => assertOnline()).toThrow(OfflineError);
  });

  it("refuses a game action without sending it while offline", async () => {
    vi.stubGlobal("navigator", { onLine: false });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const auth = { address: "GABC", signMessage: vi.fn() };

    await expect(playerAction(1, "fold", undefined, auth)).rejects.toBeInstanceOf(
      OfflineError
    );
    expect(fetchMock).not.toHaveBeenCalled();
    expect(auth.signMessage).not.toHaveBeenCalled();
  });

  it("still allows reads while offline", async () => {
    vi.stubGlobal("navigator", { onLine: false });
    const fetchMock = vi.fn(async () => new Response('{"state":"{}"}', { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(getTableState(1)).resolves.toEqual({ state: "{}" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

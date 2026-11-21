import { afterEach, describe, expect, it, vi } from "vitest";
import { getTableState, ApiRequestError } from "@/lib/api";
import {
  REQUEST_ID_HEADER,
  createRequestId,
  withSupportRef,
} from "@/lib/request-id";

const UUID_V4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("createRequestId", () => {
  it("returns a v4 UUID", () => {
    expect(createRequestId()).toMatch(UUID_V4);
  });

  it("returns a different id on every call", () => {
    const ids = new Set(Array.from({ length: 50 }, () => createRequestId()));
    expect(ids.size).toBe(50);
  });

  it("falls back to getRandomValues when randomUUID is unavailable", () => {
    vi.stubGlobal("crypto", {
      getRandomValues: (bytes: Uint8Array) => {
        bytes.fill(0xab);
        return bytes;
      },
    });
    expect(createRequestId()).toMatch(UUID_V4);
  });
});

describe("withSupportRef", () => {
  it("appends the reference to the message", () => {
    expect(withSupportRef("Deal failed: 500", "abc-123")).toBe(
      "Deal failed: 500 (ref: abc-123)"
    );
  });
});

describe("coordinator calls carry a correlation id", () => {
  it("sends X-Request-Id and surfaces the same id in the error copy", async () => {
    const fetchMock = vi.fn(
      async () =>
        new Response(JSON.stringify({ error: "table not found" }), { status: 404 })
    );
    vi.stubGlobal("fetch", fetchMock);

    const err = await getTableState(7).catch((e: unknown) => e);

    expect(err).toBeInstanceOf(ApiRequestError);
    const sent = new Headers(
      (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1].headers
    ).get(REQUEST_ID_HEADER);
    expect(sent).toMatch(UUID_V4);

    const apiErr = err as ApiRequestError;
    expect(apiErr.requestId).toBe(sent);
    expect(apiErr.status).toBe(404);
    expect(apiErr.message).toBe(`table not found (ref: ${sent})`);
  });

  it("gives each request its own id", async () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    await getTableState(1);
    await getTableState(1);

    const ids = fetchMock.mock.calls.map((call) =>
      new Headers((call as unknown as [string, RequestInit])[1].headers).get(REQUEST_ID_HEADER)
    );
    expect(ids[0]).not.toBe(ids[1]);
  });
});

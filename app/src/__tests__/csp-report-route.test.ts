import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { MAX_CSP_REPORT_BYTES } from "@/lib/csp-report";
import { POST } from "@/app/api/csp-report/route";

const validBody = JSON.stringify({
  "csp-report": {
    "violated-directive": "script-src",
    "effective-directive": "script-src",
    disposition: "report",
  },
});

function post(body: string): Promise<Response> {
  return POST(
    new Request("http://localhost:3000/api/csp-report", {
      method: "POST",
      body,
    }),
  );
}

describe("POST /api/csp-report", () => {
  const fetchMock = vi.fn();
  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    fetchMock.mockReset();
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    process.env.COORDINATOR_URL = "http://coordinator:8080/";
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    warnSpy.mockRestore();
    delete process.env.COORDINATOR_URL;
  });

  it("forwards a valid report to the coordinator and answers 204", async () => {
    const response = await post(validBody);
    expect(response.status).toBe(204);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("http://coordinator:8080/api/csp/report");
    expect(init.method).toBe("POST");
    expect(init.headers["content-type"]).toBe("application/json");
    expect(init.body).toBe(validBody);
  });

  it("forwards a valid Reporting API array", async () => {
    const body = JSON.stringify([
      {
        type: "csp",
        body: { "violated-directive": "img-src", disposition: "report" },
      },
    ]);
    const response = await post(body);
    expect(response.status).toBe(204);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0][1].body).toBe(body);
  });

  it("drops malformed reports without forwarding", async () => {
    const response = await post("not json {");
    expect(response.status).toBe(204);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("drops reports with no violation directive", async () => {
    const response = await post(
      JSON.stringify({ "csp-report": { "blocked-uri": "x" } }),
    );
    expect(response.status).toBe(204);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("drops oversized reports without forwarding", async () => {
    const oversized = JSON.stringify({
      "csp-report": { "violated-directive": "script-src" },
    }).padEnd(MAX_CSP_REPORT_BYTES + 1, " ");
    const response = await post(oversized);
    expect(response.status).toBe(204);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("answers 204 even when forwarding fails", async () => {
    fetchMock.mockRejectedValue(new Error("network down"));
    const response = await post(validBody);
    expect(response.status).toBe(204);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(warnSpy).toHaveBeenCalled();
  });
});

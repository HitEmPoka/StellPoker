import { describe, it, expect } from "vitest";
import { MAX_CSP_REPORT_BYTES, parseCspReport } from "@/lib/csp-report";

const legacyReport = JSON.stringify({
  "csp-report": {
    "document-uri": "https://app.example.com/play",
    "violated-directive": "script-src",
    "effective-directive": "script-src",
    "blocked-uri": "https://evil.example.com/x.js",
    "source-file": "https://app.example.com/bundle.js",
    disposition: "report",
    "line-number": 42,
    "column-number": 7,
    "original-policy": "default-src 'self'",
  },
});

describe("parseCspReport", () => {
  it("parses the legacy report-uri format", () => {
    const parsed = parseCspReport(legacyReport);
    expect(parsed).not.toBeNull();
    expect(parsed).toHaveLength(1);
    const violation = parsed![0];
    expect(violation.effectiveDirective).toBe("script-src");
    expect(violation.violatedDirective).toBe("script-src");
    expect(violation.blockedUri).toBe("https://evil.example.com/x.js");
    expect(violation.documentUri).toBe("https://app.example.com/play");
    expect(violation.disposition).toBe("report");
    expect(violation.lineNumber).toBe(42);
    expect(violation.columnNumber).toBe(7);
  });

  it("accepts snake_case field aliases", () => {
    const parsed = parseCspReport(
      JSON.stringify({
        "csp-report": {
          violated_directive: "img-src",
          document_uri: "https://app.example.com/",
          line_number: 3,
        },
      }),
    );
    expect(parsed).toHaveLength(1);
    expect(parsed![0].violatedDirective).toBe("img-src");
    expect(parsed![0].documentUri).toBe("https://app.example.com/");
    expect(parsed![0].lineNumber).toBe(3);
  });

  it("parses Reporting API arrays and skips non-csp entries", () => {
    const parsed = parseCspReport(
      JSON.stringify([
        { type: "network", url: "https://app.example.com/", body: {} },
        {
          type: "csp",
          age: 4,
          body: {
            "violated-directive": "style-src 'self' 'unsafe-inline'",
            disposition: "enforce",
          },
        },
      ]),
    );
    expect(parsed).toHaveLength(1);
    expect(parsed![0].violatedDirective).toBe(
      "style-src 'self' 'unsafe-inline'",
    );
    expect(parsed![0].disposition).toBe("enforce");
  });

  it("returns null for malformed JSON", () => {
    expect(parseCspReport("not json {")).toBeNull();
  });

  it("returns null for non-object bodies", () => {
    expect(parseCspReport("123")).toBeNull();
    expect(parseCspReport('"csp-report"')).toBeNull();
    expect(parseCspReport("null")).toBeNull();
  });

  it("returns null when the legacy wrapper is missing", () => {
    expect(parseCspReport(JSON.stringify({ foo: 1 }))).toBeNull();
  });

  it("returns null when no violation directive is present", () => {
    expect(
      parseCspReport(JSON.stringify({ "csp-report": { "blocked-uri": "x" } })),
    ).toBeNull();
    expect(parseCspReport(JSON.stringify([{ type: "csp", body: {} }]))).toBeNull();
  });

  it("returns null for oversized bodies", () => {
    const oversized = "[{}]".padEnd(MAX_CSP_REPORT_BYTES + 1, " ");
    expect(parseCspReport(oversized)).toBeNull();
  });

  it("returns null for empty bodies", () => {
    expect(parseCspReport("")).toBeNull();
  });

  it("caps long strings at 256 characters", () => {
    const parsed = parseCspReport(
      JSON.stringify({
        "csp-report": {
          "violated-directive": "script-src",
          "blocked-uri": "a".repeat(500),
        },
      }),
    );
    expect(parsed![0].blockedUri).toHaveLength(256);
  });

  it("drops negative or non-numeric line numbers", () => {
    const parsed = parseCspReport(
      JSON.stringify({
        "csp-report": {
          "violated-directive": "script-src",
          "line-number": -4,
          "column-number": "12",
        },
      }),
    );
    expect(parsed![0].lineNumber).toBeNull();
    expect(parsed![0].columnNumber).toBeNull();
  });

  it("normalizes dispositions outside enforce/report to unknown", () => {
    const parsed = parseCspReport(
      JSON.stringify({
        "csp-report": {
          "violated-directive": "script-src",
          disposition: "ENFORCE",
        },
      }),
    );
    expect(parsed![0].disposition).toBe("enforce");
    const unknown = parseCspReport(
      JSON.stringify({
        "csp-report": { "violated-directive": "script-src" },
      }),
    );
    expect(unknown![0].disposition).toBe("unknown");
  });
});

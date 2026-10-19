/**
 * Lobby load test: 5000 listed tables (issue #650).
 *
 * Every iteration fetches the full open-tables payload and validates both the
 * status and the number of tables. The default target is the deterministic
 * fixture in `load-testing/lobby-fixture`, which generates the payload on every
 * request so serialization is part of the measurement. Point `BASE_URL` at a
 * coordinator with `OPEN_TABLE_SCAN_MAX=5000` to exercise the real RPC-backed
 * endpoint instead.
 *
 * Recorded SLOs (see load-testing/README.md):
 *   p95 < 750 ms, p99 < 1500 ms, error rate < 1% at 50 requests/s.
 *
 * The summary is written to `RESULTS_DIR/lobby-5000-summary.json`
 * (`/scripts/results` inside the compose k6 container).
 */

import http from "k6/http";
import { check } from "k6";
import { Counter, Rate, Trend } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://lobby-fixture:8090";
const TABLE_COUNT = Number.parseInt(__ENV.TABLE_COUNT || "5000", 10);
const RATE = Number.parseInt(__ENV.RATE || "50", 10);
const DURATION = __ENV.DURATION || "1m";
const PRE_ALLOCATED_VUS = Number.parseInt(__ENV.VUS || "50", 10);
const MAX_VUS = Number.parseInt(__ENV.MAX_VUS || "200", 10);
const RESULTS_DIR = __ENV.RESULTS_DIR || "results";

const lobbyLatency = new Trend("lobby_latency_ms", true);
const lobbyPayloadBytes = new Trend("lobby_payload_bytes");
const lobbyTableCount = new Trend("lobby_table_count");
const lobbyErrors = new Rate("lobby_errors");
const lobbyRequests = new Counter("lobby_requests");

const P95_SLO_MS = 750;
const P99_SLO_MS = 1500;
const ERROR_RATE_SLO = 0.01;

export const options = {
  scenarios: {
    lobby_5000: {
      executor: "constant-arrival-rate",
      rate: RATE,
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: PRE_ALLOCATED_VUS,
      maxVUs: MAX_VUS,
      tags: { scenario: "lobby_5000" },
    },
  },
  thresholds: {
    http_req_failed: [`rate<${ERROR_RATE_SLO}`],
    lobby_errors: [`rate<${ERROR_RATE_SLO}`],
    lobby_latency_ms: [
      `p(95)<${P95_SLO_MS}`,
      `p(99)<${P99_SLO_MS}`,
    ],
    checks: ["rate>0.99"],
  },
};

function lobbyUrl() {
  return `${BASE_URL}/api/tables/open?tables=${TABLE_COUNT}`;
}

/** Warm the endpoint and prove the fixture is actually returning the target size. */
export function setup() {
  const response = http.get(lobbyUrl(), {
    tags: { name: "lobby_setup" },
  });
  const body = response.status === 200 ? response.json() : null;
  const count = body && Array.isArray(body.tables) ? body.tables.length : -1;
  if (count !== TABLE_COUNT) {
    throw new Error(
      `lobby fixture returned ${count} tables; expected ${TABLE_COUNT}. ` +
        "Check BASE_URL and TABLE_COUNT."
    );
  }
  return {
    expected_tables: TABLE_COUNT,
    payload_bytes: response.body ? response.body.length : 0,
  };
}

export default function (data) {
  const response = http.get(lobbyUrl(), {
    tags: { name: "lobby_open_5000" },
  });

  let count = -1;
  let parsed = null;
  try {
    parsed = response.json();
    if (parsed && Array.isArray(parsed.tables)) {
      count = parsed.tables.length;
    }
  } catch {
    // A non-JSON body is reported by the checks below.
  }

  lobbyLatency.add(response.timings.duration);
  lobbyPayloadBytes.add(response.body ? response.body.length : 0);
  lobbyTableCount.add(count);

  const ok = check(response, {
    "status is 200": (res) => res.status === 200,
    [`payload contains ${data.expected_tables} tables`]: () =>
      count === data.expected_tables,
    "payload is non-empty and under 10 MB": (res) =>
      !!res.body && res.body.length > 0 && res.body.length < 10 * 1024 * 1024,
  });
  lobbyErrors.add(!ok);
  lobbyRequests.add(1);
}

export function handleSummary(data) {
  const metrics = data.metrics || {};
  const values = (name) => (metrics[name] && metrics[name].values) || {};
  const latency = values("lobby_latency_ms");
  const errors = values("lobby_errors");
  const payload = values("lobby_payload_bytes");

  const summary = {
    scenario: "lobby_5000",
    table_count: TABLE_COUNT,
    target_rate_per_second: RATE,
    duration: DURATION,
    slo: {
      p95_ms: P95_SLO_MS,
      p99_ms: P99_SLO_MS,
      error_rate: ERROR_RATE_SLO,
    },
    observed: {
      requests: values("lobby_requests").count || 0,
      p50_ms: latency["p(50)"] || null,
      p95_ms: latency["p(95)"] || null,
      p99_ms: latency["p(99)"] || null,
      max_ms: latency.max || null,
      error_rate: errors.rate === undefined ? null : errors.rate,
      avg_payload_bytes: payload.avg || null,
    },
    passed:
      (latency["p(95)"] || Infinity) < P95_SLO_MS &&
      (latency["p(99)"] || Infinity) < P99_SLO_MS &&
      (errors.rate || 0) < ERROR_RATE_SLO,
    generated_at: new Date().toISOString(),
  };

  const body = `${JSON.stringify(summary, null, 2)}\n`;
  return {
    stdout: body,
    [`${RESULTS_DIR}/lobby-5000-summary.json`]: body,
  };
}

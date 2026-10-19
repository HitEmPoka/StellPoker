#!/usr/bin/env node
/**
 * Deterministic lobby fixture for the 5000-table k6 scenario (issue #650).
 *
 * The real coordinator builds `/api/tables/open` by scanning on-chain tables,
 * which makes a 5000-table fixture expensive to provision just to measure the
 * lobby read path. This dependency-free server generates the same response
 * shape on demand, so the load test can isolate serving and parsing a large
 * payload. Point `BASE_URL` at the coordinator instead when testing a deployed
 * stack that already has 5000 open tables.
 *
 * Environment:
 *   PORT         listen port (default 8090)
 *   TABLE_COUNT  number of tables in the default payload (default 5000)
 *   MAX_TABLES   largest payload a request may ask for (default 100000)
 */

import http from "node:http";

const PORT = Number.parseInt(process.env.PORT || "8090", 10);
const TABLE_COUNT = Number.parseInt(process.env.TABLE_COUNT || "5000", 10);
const MAX_TABLES = Number.parseInt(process.env.MAX_TABLES || "100000", 10);

if (!Number.isInteger(TABLE_COUNT) || TABLE_COUNT < 0) {
  throw new Error(`TABLE_COUNT must be a non-negative integer, got ${process.env.TABLE_COUNT}`);
}
if (!Number.isInteger(MAX_TABLES) || MAX_TABLES < TABLE_COUNT) {
  throw new Error(`MAX_TABLES must be >= TABLE_COUNT (${TABLE_COUNT})`);
}

/** Build the deterministic table list once; serialize per request. */
const ALL_TABLES = Array.from({ length: TABLE_COUNT }, (_, index) => {
  const seated = index % 6;
  return {
    table_id: index + 1,
    phase: "Waiting",
    max_players: 6,
    joined_wallets: seated,
    open_wallet_slots: 6 - seated,
    spectators: index % 4,
  };
});

function json(res, status, value) {
  const body = JSON.stringify(value);
  res.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "content-length": Buffer.byteLength(body),
    "cache-control": "no-store",
  });
  res.end(body);
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url || "/", `http://${req.headers.host || "localhost"}`);

  if (req.method !== "GET") {
    json(res, 405, { error: "method not allowed" });
    return;
  }

  if (url.pathname === "/api/health") {
    json(res, 200, { status: "ok", tables: TABLE_COUNT });
    return;
  }

  if (url.pathname === "/api/tables/open") {
    const requested = url.searchParams.has("tables")
      ? Number.parseInt(url.searchParams.get("tables"), 10)
      : TABLE_COUNT;
    if (!Number.isInteger(requested) || requested < 0 || requested > MAX_TABLES) {
      json(res, 400, {
        error: `tables must be between 0 and ${MAX_TABLES}`,
      });
      return;
    }

    // Serialize on every request so the test covers payload generation as well
    // as network transfer and client-side JSON parsing.
    json(res, 200, { tables: requested === TABLE_COUNT ? ALL_TABLES : ALL_TABLES.slice(0, requested) });
    return;
  }

  json(res, 404, { error: "not found" });
});

server.listen(PORT, "0.0.0.0", () => {
  console.log(
    `lobby fixture listening on :${PORT} with ${TABLE_COUNT} tables (max ${MAX_TABLES})`
  );
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    server.close(() => process.exit(0));
  });
}

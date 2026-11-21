# Coordinator support tools

## Redacted session replay

Run the replay binary with a coordinator `DATABASE_URL` and a session id:

```bash
DATABASE_URL="$DATABASE_URL" cargo run --bin replay_session -- SESSION_ID
```

The output is a JSON timeline containing phases, timestamps, public-input
digests, proof hashes, and hash-chain links. Raw shares, witnesses, request
bodies, and player secrets are never selected or printed. CI should run the
binary against its fixture database and assert that known secret fixtures do
not occur in the output.

## Crash recovery

The coordinator checkpoints in-flight sessions in the SQLite store configured
by `SESSION_RECOVERY_CACHE_DB_PATH`. On startup it scans those checkpoints and
logs resumable phases. Empty or malformed checkpoints are explicitly marked
for abort rather than retried. Operators can inspect the startup log before
re-enabling traffic after a crash.

## API deprecation schedule

Set `API_V1_SUNSET` to an RFC 7231 HTTP date, for example:

```bash
API_V1_SUNSET='Wed, 31 Dec 2026 23:59:59 GMT'
```

Requests to unversioned `/api/...` paths receive `Deprecation: true`, a
successor `Link`, and (when configured) a `Sunset` header. The admin endpoint
`GET /api/admin/deprecations` reports the legacy-request count and configured
sunset date so the schedule can be monitored before removal.

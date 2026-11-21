# Metrics Naming Convention

Every Prometheus metric emitted by any StellPoker service follows the same
shape, so dashboards, alerts, and queries port between services without
translation. This document defines the convention; `scripts/lint_metrics.py`
enforces it in CI.

The rules below are deliberately strict — the ones Prometheus itself permits
but we do not use are called out so grep-based lints can rule them out.

---

## 1. The Shape

```
<namespace>_<subsystem>_<name>_<unit>
```

| Part        | Rule                                             | Example           |
|-------------|--------------------------------------------------|-------------------|
| `namespace` | One of our fixed service namespaces (see §2).    | `coordinator`     |
| `subsystem` | Optional, lowercase, singular noun.              | `request`         |
| `name`      | Lowercase, snake_case, verb/noun of the measure. | `errors`          |
| `unit`      | Required for anything with a unit. See §3.       | `_total`, `_seconds` |

All joined with `_`, all lowercase, ASCII only, must match
`^[a-z][a-z0-9_]*[a-z0-9]$`. No double underscores.

**Full examples (good):**

- `coordinator_requests_total`
- `coordinator_request_latency_seconds`
- `coordinator_process_memory_bytes`
- `mpc_node_active_sessions`
- `mpc_node_proofs_generated_total`
- `mpc_node_cert_expiry_days`

**Bad:**

- `coordinator_RequestCount` — CamelCase.
- `coordinator-requests-total` — hyphens.
- `node_sessions` — missing namespace (collides with `node_exporter`).
- `mpc_node_proofs_generated` — counter without `_total`.
- `coordinator_latency_ms` — units in milliseconds (see §3).

---

## 2. Namespaces

| Namespace     | Owner                                   |
|---------------|-----------------------------------------|
| `coordinator` | `services/coordinator/`                 |
| `mpc_node`    | `services/node/`                        |
| `mpc_coordinator` | `services/coordinator/` (MPC-specific paths) |
| `circuits`    | Circuit compilation and proving jobs    |
| `contracts`   | On-chain reads/writes                   |
| `k6`          | Load-test produced (external)           |

Reserved prefixes (do not use): `node_` (node_exporter), `process_` (Prometheus
client auto-exports), `go_`, `promhttp_`, `prometheus_`.

Adding a new namespace requires an entry here **and** in `KNOWN_NAMESPACES`
in `scripts/lint_metrics.py`.

---

## 3. Unit Suffixes

Prometheus best practice: **base units only** (SI base, or `_total` for
monotonic counts). Rationale: dashboards do the unit conversion once, in the
display layer.

| Measure            | Suffix       | Never             |
|--------------------|--------------|-------------------|
| Monotonic counter  | `_total`     | `_count`, `_sum`  |
| Duration           | `_seconds`   | `_ms`, `_millis`, `_micros` |
| Size               | `_bytes`     | `_kb`, `_mb`      |
| Ratio (0.0–1.0)    | `_ratio`     | `_percent`, `_pct`|
| Information (time) | `_timestamp_seconds` | `_at`, `_date` |
| Count-of (gauge)   | no unit — see §4 | `_count`      |

A gauge that counts things ("number of active sessions") takes **no suffix**,
because it is a dimensionless point-in-time measurement, not an accumulator.
`mpc_node_active_sessions` is correct; `mpc_node_active_sessions_total` is
wrong (that reads as a monotonic counter).

Histogram buckets inherit the parent suffix: a `coordinator_request_latency_seconds`
histogram emits `..._seconds_bucket`, `..._seconds_sum`, `..._seconds_count` —
and that is the **only** place `_count` and `_sum` are permitted.

---

## 4. Labels

Rules:

- Labels are snake_case; same character set as metric names.
- Low cardinality only. A label whose domain is unbounded (user ID, session
  ID, raw URL path, free-form error message) is forbidden.
- No label named `type`, `kind`, or `status` on its own — those are too
  generic. Use `error_type`, `request_kind`, `http_status`.
- No label named `name` — collides with Prometheus internals in some tooling.
- Required labels for the common dimensions below are standardized so joins
  across services work:

| Dimension         | Label name       | Values                         |
|-------------------|------------------|--------------------------------|
| HTTP method       | `method`         | `get`, `post`, `put`, …        |
| HTTP status code  | `http_status`    | `200`, `400`, `500`, …         |
| Service endpoint  | `route`          | Templated path, e.g. `/tables/:id` — never the raw path |
| MPC phase         | `phase`          | `dkg`, `merge_shares`, `witness_generation`, `proof_generation` |
| Circuit name      | `circuit`        | kebab-cased circuit identifier |

Document any new standard label here before adding it to code.

---

## 5. HELP Text

Every metric declaration carries a `HELP` string that:

- Starts with a capital letter and ends with a period.
- Names the unit if ambiguous (e.g. "Days until TLS certificate expires.").
- Does not restate the metric name.

Bad: `coordinator_requests_total` → `"Coordinator requests total."`
Good: `coordinator_requests_total` → `"Total HTTP requests received by the coordinator."`

---

## 6. Retroactive Application

Existing metrics are grandfathered **only** if they already satisfy the rules.
The initial lint run audited every metric declared in `services/` and the only
accepted exceptions are listed in `scripts/lint_metrics.py` under `ALLOWLIST`
(empty by default — any entry needs a `# reason:` comment and expires on the
next major version bump).

When adding or renaming a metric, update any Grafana dashboard JSON and
alerting rule in `load-testing/prometheus-alerting-rules.yml` in the same PR.
The CI lint fails if a renamed metric name is still referenced in those files.

---

## 7. Enforcement

- Lint runs on every push via `.github/workflows/metrics-lint.yml`.
- Run locally: `python3 scripts/lint_metrics.py`.
- To scope: `python3 scripts/lint_metrics.py services/coordinator`.
- The lint checks:
  1. Shape (`^[a-z][a-z0-9_]*[a-z0-9]$`, no `__`).
  2. Namespace is in `KNOWN_NAMESPACES`.
  3. Counters end in `_total`; durations in `_seconds`; sizes in `_bytes`.
  4. No forbidden unit suffixes (`_ms`, `_percent`, `_kb`, …).
  5. HELP text present and ends in a period.
  6. Dashboards and alerting rules reference no undefined metric names.

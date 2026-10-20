# Flaky Test Detection Dashboard & Quarantine Process

This document defines the engineering standards, lifecycle, automation, and runbooks for identifying, isolating, and fixing flaky tests across the StellPoker repository.

---

## 1. Objectives & Definitions

### What is a Flaky Test?
A **flaky test** is any automated test that produces inconsistent outcomes (passing and failing) when executed on identical source code without environmental changes.

### Rerun-Only Failures
A **rerun-only failure** occurs when a test fails on its initial execution attempt in CI but passes on a subsequent retry.
- Rerun-only failures mask underlying concurrency bugs, resource leaks, or race conditions.
- Left unchecked, retries inflate CI runtimes and erode trust in the build pipeline.

### The Quarantine Guarantee
Tests confirmed as flaky are immediately segregated into the **Quarantine Registry** (`tests/quarantined_tests.json`).
- Quarantined tests do not block PR merges in the primary CI gating jobs.
- Quarantined tests continue running in a dedicated non-blocking quarantine job to gather stability telemetry.
- An automated GitHub issue is filed to ensure immediate ownership and resolution.

---

## 2. Architecture & Registry Schema

The system consists of three integrated components:
1. **Detection & Quarantine Engine:** `scripts/flaky_test_tracker.py`
2. **Quarantine Registry:** `tests/quarantined_tests.json`
3. **Flaky Test Dashboard:** `benchmark_data/flaky_dashboard.json` & `benchmark_data/flaky_dashboard.md`

### Quarantine Registry (`tests/quarantined_tests.json`)
```json
{
  "version": "1.0.0",
  "updated_at": "2026-10-20T00:00:00Z",
  "policy": {
    "auto_quarantine_threshold": 1,
    "dequarantine_consecutive_passes": 10,
    "max_quarantined_ratio": 0.05
  },
  "quarantined_tests": [
    {
      "id": "tests.test_mpc.test_peer_heartbeat_timeout",
      "file": "tests/test_mpc.py",
      "test_name": "test_peer_heartbeat_timeout",
      "quarantined_at": "2026-10-20T00:00:00Z",
      "reason": "Rerun-only failure detected across multiple runs",
      "flake_rate": 0.5,
      "errors": ["ConnectionResetError: Peer 2 heartbeat timed out"],
      "issue_url": "https://github.com/HitEmPoka/StellPoker/issues/650",
      "consecutive_passes": 0,
      "status": "quarantined"
    }
  ]
}
```

---

## 3. Quarantine Lifecycle Workflow

```
   +----------------------+
   | CI Test Run / Retries|
   +----------+-----------+
              |
              v
   +----------------------+
   | Detect Rerun-Only    |
   | Failures or Flakes   |
   +----------+-----------+
              |
      [Flake detected?]
         /         \
       Yes          No
       /             \
      v               v
+-------------+  +---------------+
| Auto-       |  | Standard Pass |
| Quarantine  |  +---------------+
+------+------+
       |
       v
+-------------+
| File GitHub |
| Issue       |
+------+------+
       |
       v
+-------------+
| Developer   |
| Fix & PR    |
+------+------+
       |
       v
+-------------+
| 10 Clean    |
| Passes      |
+------+------+
       |
       v
+-------------+
| De-quarantine|
| to Main CI  |
+-------------+
```

### Steps in Detail:
1. **Detection:** When `scripts/flaky_test_tracker.py` processes test execution records, any test exhibiting rerun-only failures or an intermittent failure rate is flagged.
2. **Quarantine:** The test entry is added to `tests/quarantined_tests.json` with status `quarantined`.
3. **Issue Creation:** An issue is filed under label `flaky-test` containing:
   - Reproduction command.
   - Stack trace and observed failure rate.
   - Runbook link and triage checklist.
4. **Triage SLA:** Flaky test issues must be acknowledged within 24 business hours and fixed or permanently retired within 7 days.
5. **De-quarantine:** Once a patch is merged and the test logs 10 consecutive clean passes in the quarantine job without retries, it is automatically eligible for promotion back to the blocking test suite.

---

## 4. Runbook for Developers

### How to reproduce a flaky test locally:
```bash
# Run the specific test 50 times in a loop
pytest tests/test_mpc.py -k test_peer_heartbeat_timeout --count=50

# For Rust contract / unit tests:
for i in {1..50}; do cargo test -p poker-table -- test_name || break; done
```

### Common Root Causes of Flakiness in StellPoker:
- **Async Timing & Tokio Sleep:** Relying on fixed duration sleeps instead of event-driven notification channels.
- **Port / Resource Contention:** Hardcoding socket ports or local RPC endpoints across parallel tests.
- **Shared Static State:** Mutating static singletons, database files, or mock registries without resetting between runs.
- **Timeouts in Proof Generation:** Cold-start CRS loading or Noir proving exceeding default test timeouts under CPU load.

---

## 5. Dashboard Inspection

The dashboard is generated automatically on every detection run:
- **JSON Data:** `benchmark_data/flaky_dashboard.json`
- **Rendered Markdown:** `benchmark_data/flaky_dashboard.md`
- **CI View:** Rendered directly to `$GITHUB_STEP_SUMMARY` in the `Flaky Test Detection & Quarantine` workflow.

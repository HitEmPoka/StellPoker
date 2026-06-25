#!/usr/bin/env python3
"""
Soak / endurance test for the StellPoker coordinator  (issue #296).

Runs a continuous stream of game actions against the coordinator for a
configurable duration (default 24 h) and reports:
  - memory / RSS growth over time
  - connection-leak indicators (fd count, open HTTP connections)
  - latency percentiles (p50 / p95 / p99)
  - error rate
  - throughput slowdown between the first and last measurement windows

Usage:
    # Quick smoke run (5 minutes)
    python3 scripts/soak-test.py --duration 300

    # Full 24-hour soak against localhost
    python3 scripts/soak-test.py

    # Against a remote coordinator
    python3 scripts/soak-test.py --base-url http://my-host:8080

    # Write a machine-readable report
    python3 scripts/soak-test.py --duration 300 --report soak-report.json

Requirements:
    pip install requests psutil
"""

import argparse
import json
import os
import random
import signal
import subprocess
import sys
import time
import threading
from collections import deque
from datetime import datetime, timezone

try:
    import requests
    from requests.adapters import HTTPAdapter
    from urllib3.util.retry import Retry
except ImportError:
    sys.exit("Missing dependency: pip install requests")

try:
    import psutil
    HAS_PSUTIL = True
except ImportError:
    HAS_PSUTIL = False
    print("[warn] psutil not found – memory/fd monitoring disabled "
          "(pip install psutil to enable)")

# ── Configuration ────────────────────────────────────────────────────────────

DEFAULT_BASE_URL   = "http://localhost:8080"
DEFAULT_DURATION_S = 24 * 3600          # 24 hours
SNAPSHOT_INTERVAL  = 60                 # collect a metric snapshot every 60 s
RESTART_INTERVAL   = 3600               # restart the coordinator every 1 h (if PID provided)
WORKER_THREADS     = 4                  # concurrent game-action workers
PLAYER_ADDRS       = [
    "PLAYER_ADDR_A000000000000000000000000000001",
    "PLAYER_ADDR_B000000000000000000000000000002",
    "PLAYER_ADDR_C000000000000000000000000000003",
    "PLAYER_ADDR_D000000000000000000000000000004",
]
PHASES             = ["flop", "turn", "river"]
ACTIONS            = ["fold", "check", "call", "bet", "raise", "all-in"]

# Thresholds that trigger a FAIL verdict in the final report.
MAX_MEMORY_GROWTH_MB  = 512     # RSS increase from baseline to end of run
MAX_ERROR_RATE_PCT    = 5.0     # % of requests that return 5xx
MAX_P99_LATENCY_MS    = 5000    # absolute p99 latency ceiling
MAX_SLOWDOWN_FACTOR   = 3.0     # throughput(start)/throughput(end) must stay below this


# ── HTTP session with retry ───────────────────────────────────────────────────

def make_session(base_url: str) -> requests.Session:
    s = requests.Session()
    retry = Retry(total=3, backoff_factor=0.3,
                  status_forcelist=[502, 503, 504])
    s.mount("http://", HTTPAdapter(max_retries=retry))
    s.mount("https://", HTTPAdapter(max_retries=retry))
    return s


# ── Metrics accumulator ───────────────────────────────────────────────────────

class Metrics:
    def __init__(self):
        self._lock = threading.Lock()
        self.ok     = 0
        self.errors = 0
        self.latencies_ms: deque = deque(maxlen=100_000)
        self.snapshots: list = []          # list of snapshot dicts

    def record(self, ok: bool, latency_ms: float):
        with self._lock:
            if ok:
                self.ok += 1
            else:
                self.errors += 1
            self.latencies_ms.append(latency_ms)

    def snapshot(self, elapsed_s: float, rss_mb: float | None, fd_count: int | None):
        with self._lock:
            total = self.ok + self.errors
            err_rate = 100.0 * self.errors / total if total else 0.0
            lats = sorted(self.latencies_ms)
            snap = {
                "elapsed_s":  round(elapsed_s, 1),
                "total_reqs": total,
                "errors":     self.errors,
                "error_rate_pct": round(err_rate, 2),
                "p50_ms":  _percentile(lats, 50),
                "p95_ms":  _percentile(lats, 95),
                "p99_ms":  _percentile(lats, 99),
                "rss_mb":  rss_mb,
                "fd_count": fd_count,
                "timestamp": datetime.now(timezone.utc).isoformat(),
            }
            self.snapshots.append(snap)
            return snap


def _percentile(sorted_data: list, pct: int) -> float | None:
    if not sorted_data:
        return None
    idx = int(len(sorted_data) * pct / 100)
    idx = min(idx, len(sorted_data) - 1)
    return round(sorted_data[idx], 1)


# ── Game action helpers ───────────────────────────────────────────────────────

def _post(session: requests.Session, url: str, body: dict) -> tuple[bool, float]:
    """Return (success, latency_ms)."""
    t0 = time.monotonic()
    try:
        r = session.post(url, json=body, timeout=30)
        ok = r.status_code < 500
    except requests.RequestException:
        ok = False
    return ok, (time.monotonic() - t0) * 1000


def _get(session: requests.Session, url: str) -> tuple[bool, float]:
    t0 = time.monotonic()
    try:
        r = session.get(url, timeout=10)
        ok = r.status_code < 500
    except requests.RequestException:
        ok = False
    return ok, (time.monotonic() - t0) * 1000


def game_cycle(base: str, session: requests.Session, metrics: Metrics):
    """
    One synthetic game cycle:
      create-table → join → request-deal → player-action(s)
                        → request-reveal(flop) → request-showdown
    All failures are recorded but never raise – the soak loop must keep going.
    """
    table_id = random.randint(1, 999_999)

    # create table
    ok, lat = _post(session, f"{base}/api/tables/create",
                    {"table_id": table_id, "players": PLAYER_ADDRS[:2]})
    metrics.record(ok, lat)

    # join
    for addr in PLAYER_ADDRS[:2]:
        ok, lat = _post(session, f"{base}/api/table/{table_id}/join",
                        {"player": addr, "buy_in": 1000})
        metrics.record(ok, lat)

    # request-deal
    ok, lat = _post(session, f"{base}/api/table/{table_id}/request-deal",
                    {"players": PLAYER_ADDRS[:2]})
    metrics.record(ok, lat)

    # player actions (random mix)
    for _ in range(random.randint(1, 3)):
        action = random.choice(ACTIONS)
        body: dict = {"action": action}
        if action in ("bet", "raise"):
            body["amount"] = random.randint(10, 500)
        ok, lat = _post(session, f"{base}/api/table/{table_id}/player-action", body)
        metrics.record(ok, lat)

    # reveal (flop)
    ok, lat = _post(session, f"{base}/api/table/{table_id}/request-reveal/flop", {})
    metrics.record(ok, lat)

    # showdown
    ok, lat = _post(session, f"{base}/api/table/{table_id}/request-showdown", {})
    metrics.record(ok, lat)

    # health poll
    ok, lat = _get(session, f"{base}/api/health")
    metrics.record(ok, lat)


# ── Worker thread ─────────────────────────────────────────────────────────────

def worker(base: str, metrics: Metrics, stop_event: threading.Event):
    session = make_session(base)
    while not stop_event.is_set():
        try:
            game_cycle(base, session, metrics)
        except Exception as exc:
            # Belt-and-suspenders: never let an uncaught exception kill a worker.
            metrics.record(False, 0.0)
            print(f"[worker] unhandled exception: {exc}", file=sys.stderr)
        # Small sleep to avoid hammering a single-node test environment.
        time.sleep(random.uniform(0.05, 0.3))


# ── System-resource helpers ───────────────────────────────────────────────────

def _get_coordinator_proc(pid: int | None):
    if not HAS_PSUTIL or pid is None:
        return None
    try:
        return psutil.Process(pid)
    except psutil.NoSuchProcess:
        return None


def _rss_mb(proc) -> float | None:
    if proc is None:
        return None
    try:
        return round(proc.memory_info().rss / 1024 / 1024, 1)
    except psutil.NoSuchProcess:
        return None


def _fd_count(proc) -> int | None:
    if proc is None:
        return None
    try:
        return proc.num_fds()
    except (psutil.NoSuchProcess, AttributeError):
        return None


# ── Restart helper ────────────────────────────────────────────────────────────

def restart_coordinator(pid: int | None, base: str) -> int | None:
    """
    Send SIGHUP to the coordinator process (causes a graceful restart when
    running under a process supervisor that re-spawns on SIGHUP).
    Returns the new PID if the process was re-found, else the original pid.
    """
    if pid is None or not HAS_PSUTIL:
        print("[restart] no PID provided – skipping restart")
        return pid
    try:
        proc = psutil.Process(pid)
        print(f"[restart] sending SIGHUP to coordinator PID {pid}")
        proc.send_signal(signal.SIGHUP)
        time.sleep(5)  # wait for restart
    except (psutil.NoSuchProcess, OSError) as e:
        print(f"[restart] {e}")
    return pid


# ── Report generation ─────────────────────────────────────────────────────────

def build_report(metrics: Metrics, baseline_rss: float | None,
                 elapsed_s: float) -> dict:
    snaps = metrics.snapshots
    first = snaps[0] if snaps else {}
    last  = snaps[-1] if snaps else {}

    # Throughput slowdown: compare first vs last 5-minute windows
    def _tps(snap):
        total = snap.get("total_reqs", 0)
        elapsed = snap.get("elapsed_s", 1)
        return total / elapsed if elapsed else 0

    first_tps = _tps(first)
    last_tps  = _tps(last)
    slowdown  = (first_tps / last_tps) if last_tps and first_tps else 1.0

    rss_end     = last.get("rss_mb")
    rss_growth  = (rss_end - baseline_rss) if (rss_end and baseline_rss) else None
    err_rate    = last.get("error_rate_pct", 0.0)
    p99         = last.get("p99_ms")

    failures = []
    if rss_growth is not None and rss_growth > MAX_MEMORY_GROWTH_MB:
        failures.append(
            f"Memory grew {rss_growth:.1f} MB > threshold {MAX_MEMORY_GROWTH_MB} MB")
    if err_rate > MAX_ERROR_RATE_PCT:
        failures.append(
            f"Error rate {err_rate:.2f}% > threshold {MAX_ERROR_RATE_PCT}%")
    if p99 is not None and p99 > MAX_P99_LATENCY_MS:
        failures.append(
            f"p99 latency {p99:.0f} ms > threshold {MAX_P99_LATENCY_MS} ms")
    if slowdown > MAX_SLOWDOWN_FACTOR:
        failures.append(
            f"Throughput slowdown {slowdown:.2f}x > threshold {MAX_SLOWDOWN_FACTOR}x")

    return {
        "verdict":          "PASS" if not failures else "FAIL",
        "failures":         failures,
        "duration_s":       round(elapsed_s, 1),
        "total_requests":   metrics.ok + metrics.errors,
        "total_errors":     metrics.errors,
        "error_rate_pct":   err_rate,
        "rss_baseline_mb":  baseline_rss,
        "rss_end_mb":       rss_end,
        "rss_growth_mb":    rss_growth,
        "final_p50_ms":     last.get("p50_ms"),
        "final_p95_ms":     last.get("p95_ms"),
        "final_p99_ms":     last.get("p99_ms"),
        "throughput_slowdown_x": round(slowdown, 2),
        "snapshots":        snaps,
    }


def print_snapshot(snap: dict):
    rss = f"{snap['rss_mb']:.1f} MB" if snap['rss_mb'] is not None else "n/a"
    fds = snap['fd_count'] if snap['fd_count'] is not None else "n/a"
    print(
        f"[{snap['elapsed_s']:>8.0f}s] "
        f"reqs={snap['total_reqs']:>7} "
        f"errs={snap['errors']:>5} ({snap['error_rate_pct']:.1f}%) "
        f"p50={snap['p50_ms'] or 0:>7.1f}ms "
        f"p99={snap['p99_ms'] or 0:>7.1f}ms "
        f"rss={rss} "
        f"fds={fds}"
    )


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Coordinator soak test")
    parser.add_argument("--base-url",  default=DEFAULT_BASE_URL)
    parser.add_argument("--duration",  type=float, default=DEFAULT_DURATION_S,
                        help="Total run time in seconds (default: 86400 = 24 h)")
    parser.add_argument("--workers",   type=int,   default=WORKER_THREADS)
    parser.add_argument("--pid",       type=int,   default=None,
                        help="PID of the coordinator process for memory/fd monitoring "
                             "and periodic restarts")
    parser.add_argument("--no-restart", action="store_true",
                        help="Disable periodic coordinator restarts")
    parser.add_argument("--report",    default=None,
                        help="Path to write the JSON report (optional)")
    args = parser.parse_args()

    base = args.base_url.rstrip("/")
    stop = threading.Event()
    metrics = Metrics()
    proc = _get_coordinator_proc(args.pid)

    print(f"[soak] Starting — url={base}  duration={args.duration}s  "
          f"workers={args.workers}  pid={args.pid}")
    print(f"[soak] Thresholds: mem<{MAX_MEMORY_GROWTH_MB}MB  "
          f"err<{MAX_ERROR_RATE_PCT}%  p99<{MAX_P99_LATENCY_MS}ms  "
          f"slowdown<{MAX_SLOWDOWN_FACTOR}x")

    # Record baseline RSS before any load.
    baseline_rss = _rss_mb(proc)
    print(f"[soak] Baseline RSS: {baseline_rss} MB")

    # Warm-up: probe health before starting workers.
    probe = make_session(base)
    try:
        r = probe.get(f"{base}/api/health", timeout=10)
        print(f"[soak] Health probe → {r.status_code}")
    except requests.RequestException as e:
        print(f"[warn] Health probe failed: {e}")

    start_time = time.monotonic()
    next_snapshot  = start_time + SNAPSHOT_INTERVAL
    next_restart   = start_time + RESTART_INTERVAL

    # Launch workers.
    threads = [
        threading.Thread(target=worker, args=(base, metrics, stop), daemon=True)
        for _ in range(args.workers)
    ]
    for t in threads:
        t.start()

    try:
        while True:
            now     = time.monotonic()
            elapsed = now - start_time

            if elapsed >= args.duration:
                break

            if now >= next_snapshot:
                snap = metrics.snapshot(elapsed, _rss_mb(proc), _fd_count(proc))
                print_snapshot(snap)
                next_snapshot = now + SNAPSHOT_INTERVAL

                # Check for catastrophic error rate mid-run.
                if snap["error_rate_pct"] > 95:
                    print("[soak] ERROR RATE > 95% — aborting run early")
                    break

            if not args.no_restart and now >= next_restart and args.pid is not None:
                proc_pid = restart_coordinator(args.pid, base)
                proc = _get_coordinator_proc(proc_pid)
                next_restart = now + RESTART_INTERVAL

            time.sleep(1)

    except KeyboardInterrupt:
        print("\n[soak] Interrupted by user")

    finally:
        stop.set()
        for t in threads:
            t.join(timeout=15)

    elapsed = time.monotonic() - start_time
    # Final snapshot.
    snap = metrics.snapshot(elapsed, _rss_mb(proc), _fd_count(proc))
    print_snapshot(snap)

    report = build_report(metrics, baseline_rss, elapsed)

    print("\n" + "=" * 60)
    print(f"  SOAK TEST RESULT: {report['verdict']}")
    print("=" * 60)
    for key in ("total_requests", "total_errors", "error_rate_pct",
                "rss_growth_mb", "final_p99_ms", "throughput_slowdown_x"):
        print(f"  {key:<30} {report[key]}")
    if report["failures"]:
        print("\n  FAILURES:")
        for f in report["failures"]:
            print(f"    • {f}")
    print("=" * 60)

    if args.report:
        with open(args.report, "w") as fh:
            json.dump(report, fh, indent=2)
        print(f"[soak] Report written to {args.report}")

    sys.exit(0 if report["verdict"] == "PASS" else 1)


if __name__ == "__main__":
    main()

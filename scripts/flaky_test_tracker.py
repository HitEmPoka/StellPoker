#!/usr/bin/env python3
"""
Flaky test detection dashboard and quarantine automation.
Tracks rerun-only test failures, generates health dashboards,
auto-quarantines flaky tests, and automates GitHub issue creation.
"""

import os
import sys
import json
import pathlib
import datetime
import argparse
import subprocess
import urllib.request
import urllib.error

REPO_ROOT = pathlib.Path(__file__).parent.parent
QUARANTINE_FILE = REPO_ROOT / "tests" / "quarantined_tests.json"
DASHBOARD_FILE = REPO_ROOT / "benchmark_data" / "flaky_dashboard.json"
DASHBOARD_MD_FILE = REPO_ROOT / "benchmark_data" / "flaky_dashboard.md"


def load_json(path: pathlib.Path, default: dict) -> dict:
    if path.exists():
        try:
            with open(path, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception as e:
            print(f"[WARN] Failed to load {path}: {e}", file=sys.stderr)
    return default


def save_json(path: pathlib.Path, data: dict):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2)
        f.write("\n")


def analyze_test_runs(runs_data: list[dict]) -> dict:
    """
    Analyze multiple test runs or multi-attempt results.
    Identifies:
      - rerun_only_failures: tests that failed in attempt 1 but succeeded on retry.
      - consistent_failures: tests that failed all attempts.
      - intermittent_flakes: tests with failure rate > 0 and < 1.
    """
    stats = {}
    for run in runs_data:
        test_id = run.get("test_id")
        file_path = run.get("file", "")
        passed = run.get("passed", False)
        attempt = run.get("attempt", 1)
        duration = run.get("duration", 0.0)
        error = run.get("error", "")

        if test_id not in stats:
            stats[test_id] = {
                "id": test_id,
                "file": file_path,
                "total_runs": 0,
                "passes": 0,
                "failures": 0,
                "attempts": [],
                "errors": []
            }

        s = stats[test_id]
        s["total_runs"] += 1
        if passed:
            s["passes"] += 1
        else:
            s["failures"] += 1
            if error and error not in s["errors"]:
                s["errors"].append(error)

        s["attempts"].append({
            "attempt": attempt,
            "passed": passed,
            "duration": duration
        })

    flaky_tests = []
    rerun_only_count = 0

    for test_id, s in stats.items():
        total = s["total_runs"]
        failures = s["failures"]
        passes = s["passes"]
        flake_rate = round(failures / total, 3) if total > 0 else 0.0

        # Detect rerun-only failure: at least one failure followed by a pass on rerun
        attempts = s["attempts"]
        is_rerun_only = False
        if len(attempts) > 1:
            first_attempt_passed = attempts[0]["passed"]
            any_later_passed = any(a["passed"] for a in attempts[1:])
            if not first_attempt_passed and any_later_passed:
                is_rerun_only = True
                rerun_only_count += 1

        if (flake_rate > 0 and passes > 0) or is_rerun_only:
            flaky_tests.append({
                "id": test_id,
                "file": s["file"],
                "total_runs": total,
                "failures": failures,
                "passes": passes,
                "flake_rate": flake_rate,
                "rerun_only_failure": is_rerun_only,
                "errors": s["errors"][:3]
            })

    return {
        "total_monitored": len(stats),
        "flaky_tests": flaky_tests,
        "rerun_only_count": rerun_only_count
    }


def update_dashboard(analysis: dict) -> dict:
    dashboard = load_json(DASHBOARD_FILE, {
        "last_updated": "",
        "summary": {},
        "flaky_tests": [],
        "history": []
    })

    quarantine = load_json(QUARANTINE_FILE, {"quarantined_tests": []})
    quarantined_ids = {t["id"] for t in quarantine.get("quarantined_tests", [])}

    now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()
    flaky_list = analysis["flaky_tests"]
    total = analysis["total_monitored"]
    flaky_count = len(flaky_list)
    rerun_count = analysis["rerun_only_count"]

    dashboard["last_updated"] = now_iso
    dashboard["summary"] = {
        "total_tests_monitored": total,
        "flaky_tests_count": flaky_count,
        "quarantined_count": len(quarantined_ids),
        "rerun_only_failures_count": rerun_count,
        "overall_flake_rate": round(flaky_count / total, 4) if total > 0 else 0.0
    }
    dashboard["flaky_tests"] = flaky_list

    # Append to rolling history (max 30 entries)
    history_entry = {
        "timestamp": now_iso,
        "flaky_count": flaky_count,
        "rerun_only_count": rerun_count
    }
    dashboard.setdefault("history", []).append(history_entry)
    dashboard["history"] = dashboard["history"][-30:]

    save_json(DASHBOARD_FILE, dashboard)
    render_markdown_dashboard(dashboard, quarantine)
    return dashboard


def render_markdown_dashboard(dashboard: dict, quarantine: dict):
    s = dashboard.get("summary", {})
    quarantined_tests = quarantine.get("quarantined_tests", [])

    lines = [
        "# Flaky Test Detection & Quarantine Dashboard",
        "",
        f"**Last Updated:** `{dashboard.get('last_updated', 'N/A')}`",
        "",
        "## Summary Metrics",
        "",
        "| Metric | Value |",
        "|---|---|",
        f"| **Total Tests Monitored** | `{s.get('total_tests_monitored', 0)}` |",
        f"| **Flaky Tests Detected** | `{s.get('flaky_tests_count', 0)}` |",
        f"| **Rerun-Only Failures** | `{s.get('rerun_only_failures_count', 0)}` |",
        f"| **Active Quarantined Tests** | `{s.get('quarantined_count', 0)}` |",
        f"| **Overall Flake Rate** | `{s.get('overall_flake_rate', 0.0) * 100:.1f}%` |",
        "",
        "## Detected Flaky Tests",
        ""
    ]

    flaky_tests = dashboard.get("flaky_tests", [])
    if flaky_tests:
        lines.append("| Test ID | File | Flake Rate | Rerun-Only Failure | Failure Snippet |")
        lines.append("|---|---|---|---|---|")
        for t in flaky_tests:
            err = t.get("errors", [""])[0].replace("\n", " ")[:60] if t.get("errors") else "None"
            lines.append(f"| `{t['id']}` | `{t['file']}` | `{t['flake_rate'] * 100:.1f}%` | `{'Yes' if t['rerun_only_failure'] else 'No'}` | `{err}` |")
    else:
        lines.append("🎉 *No flaky tests detected in recent runs.*")

    lines.extend([
        "",
        "## Quarantined Tests Registry",
        ""
    ])

    if quarantined_tests:
        lines.append("| Test ID | Quarantined At | Reason | Flake Rate | Issue URL | Status |")
        lines.append("|---|---|---|---|---|---|")
        for q in quarantined_tests:
            lines.append(f"| `{q['id']}` | `{q.get('quarantined_at', 'N/A')}` | {q.get('reason', '')} | `{q.get('flake_rate', 0)*100:.1f}%` | [Issue]({q.get('issue_url', '#')}) | `{q.get('status', 'quarantined')}` |")
    else:
        lines.append("✅ *Quarantine registry is empty. All tests are running in standard blocking gate.*")

    lines.append("")
    DASHBOARD_MD_FILE.parent.mkdir(parents=True, exist_ok=True)
    with open(DASHBOARD_MD_FILE, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


def auto_quarantine(flaky_tests: list[dict], threshold: float = 0.0) -> list[dict]:
    """Auto-quarantine flaky tests exceeding threshold into quarantine registry."""
    quarantine = load_json(QUARANTINE_FILE, {
        "version": "1.0.0",
        "updated_at": "",
        "policy": {
            "auto_quarantine_threshold": 1,
            "dequarantine_consecutive_passes": 10
        },
        "quarantined_tests": []
    })

    existing_ids = {t["id"]: t for t in quarantine.get("quarantined_tests", [])}
    newly_quarantined = []
    now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat()

    for t in flaky_tests:
        if t["flake_rate"] > threshold or t.get("rerun_only_failure"):
            test_id = t["id"]
            if test_id not in existing_ids:
                entry = {
                    "id": test_id,
                    "file": t["file"],
                    "test_name": test_id.split(".")[-1],
                    "quarantined_at": now_iso,
                    "reason": "Rerun-only failure detected across multiple runs" if t.get("rerun_only_failure") else f"Flake rate {t['flake_rate']*100:.1f}% exceeds threshold",
                    "flake_rate": t["flake_rate"],
                    "errors": t.get("errors", []),
                    "issue_url": "",
                    "consecutive_passes": 0,
                    "status": "quarantined"
                }
                quarantine["quarantined_tests"].append(entry)
                newly_quarantined.append(entry)
                print(f"[QUARANTINE] Auto-quarantined flaky test: {test_id} (flake rate: {t['flake_rate']})")

    if newly_quarantined:
        quarantine["updated_at"] = now_iso
        save_json(QUARANTINE_FILE, quarantine)

    return newly_quarantined


def create_github_issues_for_quarantined(newly_quarantined: list[dict]):
    """Create GitHub issues for newly quarantined tests using GitHub API or gh CLI."""
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    repo = os.environ.get("GITHUB_REPOSITORY", "HitEmPoka/StellPoker")

    for q in newly_quarantined:
        title = f"[Flaky Test Quarantine] {q['id']}"
        body = f"""### Flaky Test Detection Report

**Test ID:** `{q['id']}`
**File:** `{q['file']}`
**Quarantined At:** `{q['quarantined_at']}`
**Reason:** {q['reason']}
**Observed Flake Rate:** `{q['flake_rate'] * 100:.1f}%`

#### Failure Details:
```text
{json.dumps(q.get('errors', []), indent=2)}
```

#### Runbook & Next Steps:
1. Review the failure trace and identify nondeterminism (timing, concurrency, shared state, network).
2. Reproduce locally: `pytest {q['file']} -k {q['test_name']} --count=50`
3. Push a fix and observe clean passes in CI.
4. When verified (10 consecutive clean passes), the test will be de-quarantined.

See [docs/flaky-test-quarantine.md](docs/flaky-test-quarantine.md) for full process.
"""
        # Try using gh CLI first
        issue_created = False
        try:
            cmd = ["gh", "issue", "create", "--repo", repo, "--title", title, "--body", body, "--label", "flaky-test"]
            res = subprocess.run(cmd, capture_output=True, text=True)
            if res.returncode == 0:
                issue_url = res.stdout.strip()
                q["issue_url"] = issue_url
                issue_created = True
                print(f"[ISSUE] Created issue via gh CLI: {issue_url}")
        except Exception:
            pass

        if not issue_created and token:
            # Try REST API directly
            api_url = f"https://api.github.com/repos/{repo}/issues"
            payload = json.dumps({"title": title, "body": body, "labels": ["flaky-test"]}).encode("utf-8")
            req = urllib.request.Request(
                api_url,
                data=payload,
                headers={
                    "Authorization": f"Bearer {token}",
                    "Accept": "application/vnd.github+json",
                    "Content-Type": "application/json"
                }
            )
            try:
                with urllib.request.urlopen(req) as resp:
                    resp_data = json.loads(resp.read().decode("utf-8"))
                    q["issue_url"] = resp_data.get("html_url", "")
                    issue_created = True
                    print(f"[ISSUE] Created issue via GitHub API: {q['issue_url']}")
            except Exception as e:
                print(f"[WARN] Failed to create GitHub issue via API: {e}", file=sys.stderr)

        if not issue_created:
            # Fallback stub URL
            q["issue_url"] = f"https://github.com/{repo}/issues/new?title={urllib.parse.quote(title)}"

    # Update quarantine registry with issue URLs
    if newly_quarantined:
        quarantine = load_json(QUARANTINE_FILE, {"quarantined_tests": []})
        for qt in quarantine.get("quarantined_tests", []):
            for nq in newly_quarantined:
                if qt["id"] == nq["id"] and nq.get("issue_url"):
                    qt["issue_url"] = nq["issue_url"]
        save_json(QUARANTINE_FILE, quarantine)


def main() -> int:
    parser = argparse.ArgumentParser(description="Flaky Test Detection & Quarantine Manager")
    parser.add_argument("--input-results", help="Path to JSON file containing test run results")
    parser.add_argument("--auto-quarantine", action="store_true", help="Auto-quarantine detected flaky tests")
    parser.add_argument("--create-issues", action="store_true", help="Open GitHub issues for newly quarantined tests")
    parser.add_argument("--render-only", action="store_true", help="Render Markdown dashboard from current JSON")
    parser.add_argument("--simulate-sample", action="store_true", help="Run simulated test suite with flaky test for verification")
    args = parser.parse_args()

    if args.render_only:
        dashboard = load_json(DASHBOARD_FILE, {})
        quarantine = load_json(QUARANTINE_FILE, {})
        render_markdown_dashboard(dashboard, quarantine)
        print(f"[OK] Rendered markdown dashboard at {DASHBOARD_MD_FILE}")
        return 0

    runs_data = []
    if args.input_results and pathlib.Path(args.input_results).exists():
        with open(args.input_results, "r", encoding="utf-8") as f:
            runs_data = json.load(f)
    elif args.simulate_sample:
        # Sample simulated run data with 1 flaky rerun-only failure and 4 clean tests
        runs_data = [
            {"test_id": "tests.test_circuit.test_proof_generation", "file": "tests/test_circuit.py", "attempt": 1, "passed": True, "duration": 1.2},
            {"test_id": "tests.test_contracts.test_table_settlement", "file": "tests/test_contracts.py", "attempt": 1, "passed": True, "duration": 0.5},
            {"test_id": "tests.test_api.test_status_endpoint", "file": "tests/test_api.py", "attempt": 1, "passed": True, "duration": 0.1},
            {"test_id": "tests.test_mpc.test_peer_heartbeat_timeout", "file": "tests/test_mpc.py", "attempt": 1, "passed": False, "duration": 2.5, "error": "ConnectionResetError: Peer 2 heartbeat timed out"},
            {"test_id": "tests.test_mpc.test_peer_heartbeat_timeout", "file": "tests/test_mpc.py", "attempt": 2, "passed": True, "duration": 1.1}
        ]
    else:
        print("[INFO] No input results provided; rendering existing dashboard state.")
        dashboard = load_json(DASHBOARD_FILE, {})
        quarantine = load_json(QUARANTINE_FILE, {})
        render_markdown_dashboard(dashboard, quarantine)
        return 0

    analysis = analyze_test_runs(runs_data)
    dashboard = update_dashboard(analysis)

    print(f"Total Monitored: {analysis['total_monitored']}")
    print(f"Flaky Tests: {len(analysis['flaky_tests'])}")
    print(f"Rerun-Only Failures: {analysis['rerun_only_count']}")

    if args.auto_quarantine:
        newly_quarantined = auto_quarantine(analysis["flaky_tests"])
        if args.create_issues and newly_quarantined:
            create_github_issues_for_quarantined(newly_quarantined)

    return 0


if __name__ == "__main__":
    sys.exit(main())

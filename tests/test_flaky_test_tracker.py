"""
Unit tests for flaky test detection and quarantine automation.
"""

import unittest
import importlib.util
import pathlib
import tempfile
import json
import sys
from unittest import mock

SCRIPT_PATH = pathlib.Path(__file__).parent.parent / "scripts" / "flaky_test_tracker.py"


class TestFlakyTestTracker(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("flaky_test_tracker", str(SCRIPT_PATH))
        cls.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.module)

    def test_analyze_test_runs_rerun_only_detection(self):
        runs = [
            {"test_id": "test_clean", "file": "test_1.py", "attempt": 1, "passed": True},
            {"test_id": "test_flaky", "file": "test_2.py", "attempt": 1, "passed": False, "error": "TimeoutError"},
            {"test_id": "test_flaky", "file": "test_2.py", "attempt": 2, "passed": True},
        ]
        res = self.module.analyze_test_runs(runs)
        self.assertEqual(res["total_monitored"], 2)
        self.assertEqual(res["rerun_only_count"], 1)
        self.assertEqual(len(res["flaky_tests"]), 1)

        flaky = res["flaky_tests"][0]
        self.assertEqual(flaky["id"], "test_flaky")
        self.assertTrue(flaky["rerun_only_failure"])
        self.assertEqual(flaky["flake_rate"], 0.5)

    def test_auto_quarantine_logic(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            temp_q_path = pathlib.Path(tmpdir) / "quarantined_tests.json"
            orig_quarantine = self.module.QUARANTINE_FILE
            try:
                self.module.QUARANTINE_FILE = temp_q_path
                flaky_tests = [{
                    "id": "tests.test_circuit.test_prove_deal",
                    "file": "tests/test_circuit.py",
                    "flake_rate": 0.33,
                    "rerun_only_failure": True,
                    "errors": ["AssertionError: Witness mismatch"]
                }]
                newly = self.module.auto_quarantine(flaky_tests, threshold=0.0)
                self.assertEqual(len(newly), 1)
                self.assertEqual(newly[0]["id"], "tests.test_circuit.test_prove_deal")

                # Verify persistent JSON
                with open(temp_q_path, "r", encoding="utf-8") as f:
                    data = json.load(f)
                self.assertEqual(len(data["quarantined_tests"]), 1)
                self.assertEqual(data["quarantined_tests"][0]["id"], "tests.test_circuit.test_prove_deal")
            finally:
                self.module.QUARANTINE_FILE = orig_quarantine


    def test_auto_quarantine_dry_run_does_not_persist(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            temp_q_path = pathlib.Path(tmpdir) / "quarantined_tests.json"
            orig_quarantine = self.module.QUARANTINE_FILE
            try:
                self.module.QUARANTINE_FILE = temp_q_path
                flaky_tests = [{
                    "id": "tests.test_mpc.test_peer_heartbeat_timeout",
                    "file": "tests/test_mpc.py",
                    "flake_rate": 0.5,
                    "rerun_only_failure": True,
                    "errors": ["ConnectionResetError: Peer 2 heartbeat timed out"],
                }]
                newly = self.module.auto_quarantine(flaky_tests, persist=False)
                self.assertEqual(len(newly), 1)
                self.assertFalse(temp_q_path.exists())
            finally:
                self.module.QUARANTINE_FILE = orig_quarantine

    def test_simulate_sample_never_creates_issues(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = pathlib.Path(tmpdir)
            originals = (self.module.QUARANTINE_FILE, self.module.DASHBOARD_FILE, self.module.DASHBOARD_MD_FILE)
            try:
                self.module.QUARANTINE_FILE = tmp / "quarantined_tests.json"
                self.module.DASHBOARD_FILE = tmp / "flaky_dashboard.json"
                self.module.DASHBOARD_MD_FILE = tmp / "flaky_dashboard.md"
                argv = ["flaky_test_tracker.py", "--auto-quarantine", "--create-issues", "--simulate-sample"]
                with mock.patch.object(sys, "argv", argv), \
                        mock.patch.object(self.module, "create_github_issues_for_quarantined") as create_issues, \
                        mock.patch.object(self.module.subprocess, "run") as run:
                    self.assertEqual(self.module.main(), 0)
                create_issues.assert_not_called()
                run.assert_not_called()
                self.assertFalse(self.module.QUARANTINE_FILE.exists())
            finally:
                (self.module.QUARANTINE_FILE, self.module.DASHBOARD_FILE, self.module.DASHBOARD_MD_FILE) = originals

    def test_existing_open_issue_is_reused(self):
        title = self.module.issue_title_for("tests.test_mpc.test_peer_heartbeat_timeout")
        url = "https://github.com/HitEmPoka/StellPoker/issues/816"
        listing = json.dumps([{"title": title, "url": url}])
        entry = {
            "id": "tests.test_mpc.test_peer_heartbeat_timeout",
            "file": "tests/test_mpc.py",
            "test_name": "test_peer_heartbeat_timeout",
            "quarantined_at": "2026-11-18T00:00:00+00:00",
            "reason": "Rerun-only failure detected across multiple runs",
            "flake_rate": 0.5,
            "errors": [],
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            orig_quarantine = self.module.QUARANTINE_FILE
            try:
                self.module.QUARANTINE_FILE = pathlib.Path(tmpdir) / "quarantined_tests.json"
                completed = mock.Mock(returncode=0, stdout=listing)
                with mock.patch.object(self.module.subprocess, "run", return_value=completed) as run:
                    self.module.create_github_issues_for_quarantined([entry])
                self.assertEqual(run.call_count, 1)
                self.assertEqual(run.call_args[0][0][:3], ["gh", "issue", "list"])
                self.assertEqual(entry["issue_url"], url)
            finally:
                self.module.QUARANTINE_FILE = orig_quarantine

    def test_find_existing_issue_requires_exact_title(self):
        title = self.module.issue_title_for("tests.test_mpc.test_peer_heartbeat_timeout")
        listing = json.dumps([{"title": title + "_other", "url": "https://example.invalid/1"}])
        completed = mock.Mock(returncode=0, stdout=listing)
        with mock.patch.object(self.module.subprocess, "run", return_value=completed):
            self.assertIsNone(self.module.find_existing_issue("HitEmPoka/StellPoker", title, None))


if __name__ == "__main__":
    unittest.main()

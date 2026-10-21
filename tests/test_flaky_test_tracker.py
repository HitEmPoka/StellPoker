"""
Unit tests for flaky test detection and quarantine automation.
"""

import unittest
import importlib.util
import pathlib
import tempfile
import json

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


if __name__ == "__main__":
    unittest.main()

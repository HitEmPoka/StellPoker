"""
Test suite for Soroban RPC contract integration.
Exercises client RPC contracts, retry mechanisms, and stability guarantees.
"""

import unittest
import importlib.util
import pathlib

SCRIPT_PATH = pathlib.Path(__file__).parent.parent / "scripts" / "test_contract_integration_rpc.py"


class TestSorobanRpcContractIntegration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("test_contract_integration_rpc", str(SCRIPT_PATH))
        cls.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.module)

    def test_mock_protocol_validation(self):
        """Verifies full RPC contract test execution against deterministic test harness."""
        ok = self.module.run_mock_validation()
        self.assertTrue(ok, "Soroban RPC contract protocol verification failed")

    def test_client_initialization_defaults(self):
        client = self.module.SorobanRpcClient()
        self.assertEqual(client.rpc_url, self.module.DEFAULT_RPC_URL)
        self.assertEqual(client.request_id, 0)


if __name__ == "__main__":
    unittest.main()

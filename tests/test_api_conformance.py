"""API contract tests between the OpenAPI specification and coordinator router."""

import importlib.util
import pathlib
import unittest

try:
    import yaml
except ImportError:  # PyYAML is an optional test dependency
    yaml = None

REPO_ROOT = pathlib.Path(__file__).parent.parent
SPEC_PATH = REPO_ROOT / "docs" / "openapi.yaml"
EXCLUSIONS_PATH = REPO_ROOT / "docs" / "api" / "route-exclusions.yaml"
COORDINATOR_MAIN_RS = REPO_ROOT / "services" / "coordinator" / "src" / "main.rs"
CHECK_SCRIPT = REPO_ROOT / "scripts" / "check-api-spec.py"


def load_checker():
    """Load the hyphenated check script as a module so CI and pytest share logic."""
    module_spec = importlib.util.spec_from_file_location("check_api_spec", CHECK_SCRIPT)
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


checker = load_checker()


class TestApiConformance(unittest.TestCase):
    def test_openapi_spec_exists(self):
        self.assertTrue(SPEC_PATH.exists(), f"OpenAPI spec missing at {SPEC_PATH}")
        self.assertTrue(
            EXCLUSIONS_PATH.exists(),
            f"Route exclusion manifest missing at {EXCLUSIONS_PATH}",
        )

    @unittest.skipIf(yaml is None, "PyYAML not installed")
    def test_openapi_spec_structure(self):
        with open(SPEC_PATH, "r", encoding="utf-8") as f:
            spec = yaml.safe_load(f)

        for required_key in ("openapi", "info", "paths"):
            self.assertIn(required_key, spec, f"Missing required top-level key: {required_key}")

        paths = spec.get("paths", {})
        self.assertGreaterEqual(len(paths), 15, f"Expected at least 15 endpoints in spec, found {len(paths)}")

        for path, path_item in paths.items():
            self.assertTrue(path.startswith("/"), f"Path must start with '/': {path}")
            for method in ("get", "post", "put", "patch", "delete"):
                op = path_item.get(method)
                if op is not None:
                    self.assertIn("responses", op, f"Method {method.upper()} {path} missing responses block")
                    self.assertIn("operationId", op, f"Method {method.upper()} {path} missing operationId")

    @unittest.skipIf(yaml is None, "PyYAML not installed")
    def test_exclusion_manifest_is_valid(self):
        loaded = checker.load_route_exclusions()
        self.assertIsNotNone(loaded, "Route exclusion manifest failed validation")
        excluded, categories = loaded
        self.assertTrue(categories, "Exclusion categories must have written rationales")
        self.assertTrue(excluded, "At least one internal route should have a documented exclusion")
        for method, path in excluded:
            self.assertIn(method, checker.HTTP_METHODS)
            self.assertTrue(path.startswith("/"))

    @unittest.skipIf(yaml is None, "PyYAML not installed")
    def test_coordinator_handler_parity_with_openapi(self):
        """
        Zero drift in both directions:

        - every OpenAPI operation has a router registration,
        - every registered router operation is documented or explicitly
          excluded with a written rationale,
        - neither the spec nor the exclusion manifest contains stale entries.
        """
        self.assertTrue(
            COORDINATOR_MAIN_RS.exists(),
            f"Coordinator main.rs missing at {COORDINATOR_MAIN_RS}",
        )
        spec = checker.load_spec()
        self.assertTrue(checker.validate_handler_parity(spec))


if __name__ == "__main__":
    unittest.main()

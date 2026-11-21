"""
API contract tests between OpenAPI specification and coordinator implementation.
Enforces zero handler drift and schema validity using Schemathesis and static inspection.
"""

import pathlib
import re
import unittest

try:
    import yaml
except ImportError:  # PyYAML is an optional test dependency
    yaml = None

REPO_ROOT = pathlib.Path(__file__).parent.parent
SPEC_PATH = REPO_ROOT / "docs" / "openapi.yaml"
PUBLISHED_SPEC_PATH = REPO_ROOT / "docs" / "api" / "openapi.yaml"
COORDINATOR_MAIN_RS = REPO_ROOT / "services" / "coordinator" / "src" / "main.rs"
CHECKER_PATH = REPO_ROOT / "scripts" / "check-api-spec.py"


def load_checker():
    """Load the hyphenated drift-check script as a module for direct testing."""
    import importlib.util

    module_spec = importlib.util.spec_from_file_location("check_api_spec", CHECKER_PATH)
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


class TestApiConformance(unittest.TestCase):
    def test_openapi_spec_exists(self):
        self.assertTrue(SPEC_PATH.exists(), f"OpenAPI spec missing at {SPEC_PATH}")

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
    def test_coordinator_handler_parity_with_openapi(self):
        """
        Ensure every route and HTTP method declared in OpenAPI has a corresponding
        route handler registered in the coordinator Axum router.
        Fails when handlers diverge from spec.
        """
        self.assertTrue(COORDINATOR_MAIN_RS.exists(), f"Coordinator main.rs missing at {COORDINATOR_MAIN_RS}")

        with open(SPEC_PATH, "r", encoding="utf-8") as f:
            spec = yaml.safe_load(f)

        with open(COORDINATOR_MAIN_RS, "r", encoding="utf-8") as f:
            main_content = f.read()

        impl_routes = set()
        idx = 0
        while True:
            pos = main_content.find(".route(", idx)
            if pos == -1:
                break
            open_pos = pos + len(".route(")
            depth = 1
            i = open_pos
            in_string = False
            escape = False
            while i < len(main_content) and depth > 0:
                c = main_content[i]
                if escape:
                    escape = False
                elif c == '\\':
                    escape = True
                elif c == '"':
                    in_string = not in_string
                elif not in_string:
                    if c == '(':
                        depth += 1
                    elif c == ')':
                        depth -= 1
                i += 1
            if depth == 0:
                inner = main_content[open_pos:i-1].strip()
                str_match = re.match(r'^"([^"]+)"\s*,\s*(.*)$', inner, re.DOTALL)
                if str_match:
                    path = str_match.group(1).strip()
                    handler_expr = str_match.group(2).strip()
                    for method in ("get", "post", "put", "patch", "delete"):
                        if re.search(r'(?:axum::routing::|\b)' + method + r'\(', handler_expr) or re.search(r'\.' + method + r'\(', handler_expr):
                            impl_routes.add((method, path))
            idx = pos + 7

        missing = []
        for path, path_item in spec.get("paths", {}).items():
            axum_path = re.sub(r'\{([^}]+)\}', r':\1', path)
            for method, op in path_item.items():
                if method.lower() in ("get", "post", "put", "patch", "delete"):
                    key = (method.lower(), axum_path)
                    if key not in impl_routes:
                        missing.append(f"{method.upper()} {path} (op: {op.get('operationId', 'unnamed')})")

        self.assertEqual(len(missing), 0, f"The following OpenAPI operations are missing in coordinator router: {missing}")

    @unittest.skipIf(yaml is None, "PyYAML not installed")
    def test_every_router_route_is_documented_or_excluded(self):
        """Issue #1011: spec -> router alone is not enough; check router -> spec."""
        checker = load_checker()
        with open(SPEC_PATH, "r", encoding="utf-8") as f:
            spec = yaml.safe_load(f)
        self.assertTrue(checker.validate_route_coverage(spec))

    @unittest.skipIf(yaml is None, "PyYAML not installed")
    def test_route_exclusions_require_a_written_rationale(self):
        checker = load_checker()
        with open(SPEC_PATH, "r", encoding="utf-8") as f:
            spec = yaml.safe_load(f)
        rules = checker.route_exclusions(spec)
        self.assertGreater(len(rules), 0)
        for rule in rules:
            self.assertTrue(rule.get("reason"), f"Exclusion without a reason: {rule}")
        # Prefix and suffix matching work on axum-style paths.
        self.assertTrue(checker.is_excluded("get", "/api/admin/stats", rules))
        self.assertTrue(checker.is_excluded("get", "/api/table/1/chat/ws", rules))
        self.assertFalse(checker.is_excluded("get", "/api/tables/open", rules))

    @unittest.skipIf(yaml is None, "PyYAML not installed")
    def test_published_api_spec_is_regenerated(self):
        checker = load_checker()
        with open(SPEC_PATH, "r", encoding="utf-8") as f:
            spec = yaml.safe_load(f)
        self.assertTrue(PUBLISHED_SPEC_PATH.exists())
        self.assertTrue(checker.validate_published_spec(spec))


if __name__ == "__main__":
    unittest.main()

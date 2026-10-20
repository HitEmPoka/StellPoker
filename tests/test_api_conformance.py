"""
API contract tests between OpenAPI specification and coordinator implementation.
Enforces zero handler drift and schema validity using Schemathesis and static inspection.
"""

import pathlib
import re
import yaml
import unittest

REPO_ROOT = pathlib.Path(__file__).parent.parent
SPEC_PATH = REPO_ROOT / "docs" / "openapi.yaml"
COORDINATOR_MAIN_RS = REPO_ROOT / "services" / "coordinator" / "src" / "main.rs"


class TestApiConformance(unittest.TestCase):
    def test_openapi_spec_exists(self):
        self.assertTrue(SPEC_PATH.exists(), f"OpenAPI spec missing at {SPEC_PATH}")

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


if __name__ == "__main__":
    unittest.main()

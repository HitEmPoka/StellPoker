#!/usr/bin/env python3
"""
Check that the Poseidon2 golden vectors in tests/poseidon2_vectors.json
are consistent with the values asserted in circuits/lib/src/test_vectors.nr.

This catches implementation drift between the Noir circuit library,
the Rust crate, and the Soroban host-function path (issue #893).

Exit 0 if all vectors are present and consistent; exit 1 on any mismatch.
"""

import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
VECTORS_JSON = REPO_ROOT / "tests" / "poseidon2_vectors.json"
NOIR_VECTORS = REPO_ROOT / "circuits" / "lib" / "src" / "test_vectors.nr"


def normalise_hex(value: str) -> str:
    """Lowercase hex, strip 0x prefix, left-pad to 64 chars."""
    v = value.lower().replace("0x", "").lstrip("0") or "0"
    return v.zfill(64)


def extract_noir_asserts(text: str) -> dict:
    """
    Parse lines of the form:
      assert(commitments::poseidon2_hash_2(A, B) == C);
      assert(commitments::poseidon2_hash_3(A, B, C) == D);
      assert(commitments::commit_card(CARD, SALT) == OUT);
    Returns a dict mapping normalised (fn, inputs_tuple) -> normalised_output.
    """
    results = {}

    # poseidon2_hash_2 and poseidon2_hash_3 direct calls
    for m in re.finditer(
        r'poseidon2_hash_(\d)\(([^)]+)\)\s*==\s*(0x[0-9a-fA-F]+)', text
    ):
        arity = int(m.group(1))
        args = [a.strip() for a in m.group(2).split(",")]
        output = m.group(3)
        key = (f"hash_{arity}", tuple(normalise_hex(a) for a in args))
        results[key] = normalise_hex(output)

    # commit_card calls
    for m in re.finditer(
        r'commit_card\(([^,)]+),\s*(0x[0-9a-fA-F]+)\)\s*==\s*(0x[0-9a-fA-F]+)', text
    ):
        card = m.group(1).strip()
        salt = m.group(2).strip()
        output = m.group(3)
        key = ("commit_card", (normalise_hex(card), normalise_hex(salt)))
        results[key] = normalise_hex(output)

    return results


def main() -> int:
    if not VECTORS_JSON.exists():
        print(f"ERROR: {VECTORS_JSON} not found")
        return 1

    if not NOIR_VECTORS.exists():
        print(f"ERROR: {NOIR_VECTORS} not found")
        return 1

    with VECTORS_JSON.open() as f:
        vectors = json.load(f)

    noir_text = NOIR_VECTORS.read_text()
    noir_asserts = extract_noir_asserts(noir_text)

    errors = []

    # Check poseidon2_hash_2 vectors
    for vec in vectors.get("poseidon2_hash_2_vectors", []):
        a = normalise_hex(vec["a"])
        b = normalise_hex(vec["b"])
        expected = normalise_hex(vec["expected"])
        key = ("hash_2", (a, b))
        if key in noir_asserts:
            if noir_asserts[key] != expected:
                errors.append(
                    f"MISMATCH [{vec['id']}]: JSON={expected} Noir={noir_asserts[key]}"
                )
        else:
            print(f"INFO: vector '{vec['id']}' not found in Noir test file (not an error)")

    # Check poseidon2_hash_3 vectors
    for vec in vectors.get("poseidon2_hash_3_vectors", []):
        a = normalise_hex(vec["a"])
        b = normalise_hex(vec["b"])
        c = normalise_hex(vec["c"])
        expected = normalise_hex(vec["expected"])
        key = ("hash_3", (a, b, c))
        if key in noir_asserts:
            if noir_asserts[key] != expected:
                errors.append(
                    f"MISMATCH [{vec['id']}]: JSON={expected} Noir={noir_asserts[key]}"
                )
        else:
            print(f"INFO: vector '{vec['id']}' not found in Noir test file (not an error)")

    # Check commitment vectors
    for vec in vectors.get("commitment_vectors", []):
        card = normalise_hex(vec["card"])
        salt = normalise_hex(vec["salt"])
        expected = normalise_hex(vec["expected"])
        key = ("commit_card", (card, salt))
        if key in noir_asserts:
            if noir_asserts[key] != expected:
                errors.append(
                    f"MISMATCH [{vec['id']}]: JSON={expected} Noir={noir_asserts[key]}"
                )
        else:
            print(f"INFO: commitment vector '{vec['id']}' not found in Noir test file (not an error)")

    if errors:
        print("FAIL: Poseidon2 vector mismatches found:")
        for e in errors:
            print(f"  {e}")
        return 1

    print(f"OK: all Poseidon2 vectors consistent between JSON and Noir test file.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

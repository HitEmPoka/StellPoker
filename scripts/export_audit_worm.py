#!/usr/bin/env python3
"""Export the coordinator audit log to append-only (WORM) storage (Issue #681).

Reads the tamper-evident `audit_logs` table, verifies the SHA-256 hash chain
(same algorithm as services/coordinator/src/audit_log.rs), and writes the
records to an append-only JSONL file plus an integrity manifest with a
retention policy. Exports fail closed: no export is written when the chain
does not verify.

Usage:
    DATABASE_URL=postgres://... ./scripts/export_audit_worm.py
    ./scripts/export_audit_worm.py --output-dir /var/lib/worm/audit-logs
    ./scripts/export_audit_worm.py --self-test
"""

import argparse
import contextlib
import hashlib
import io
import json
import os
import subprocess
import sys
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROW_QUERY = """
SELECT id, request_id, timestamp, requester_address, action, endpoint, method,
       ip_address, response_status, error_message, table_id, session_id,
       previous_hash, record_hash
FROM audit_logs
ORDER BY id ASC
"""

DEFAULT_RETENTION_DAYS = 365

# Reference vectors shared with the Rust test in audit_log.rs
# (record_hash_matches_reference_vectors). Both sides must produce these.
FIXTURE_A = {
    "request_id": "0195e5a1-2b3c-4d5e-8f70-a1b2c3d4e5f6",
    "timestamp": "2026-03-04T05:06:07.123456+00:00",
    "requester_address": "GADMIN",
    "action": "session.create",
    "endpoint": "/api/sessions",
    "method": "POST",
    "ip_address": "203.0.113.7",
    "response_status": 201,
    "previous_hash": None,
}
FIXTURE_A_HASH = "2c5eb519c3546e3574718dd80208693ccc3882b8d700156fdacfa9f65bfb9204"
FIXTURE_B = {
    **FIXTURE_A,
    "timestamp": "2026-03-04T05:06:07.100000+00:00",
    "action": "session.denied",
    "response_status": 401,
    "previous_hash": FIXTURE_A_HASH,
}
FIXTURE_B_HASH = "1de99845fe8b3c18f658c3c6d4a49cff52d6e4503167d763be848c1150eb87b8"


def chrono_rfc3339(value):
    """Format a timestamp like Rust chrono's DateTime<Utc>::to_rfc3339().

    Postgres timestamptz has microsecond precision; chrono trims the fraction
    to milliseconds when the microseconds allow it (.100000 -> .100).
    """
    if isinstance(value, str):
        value = value.replace("Z", "+00:00")
        dt = datetime.fromisoformat(value)
    else:
        dt = value
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    dt = dt.astimezone(timezone.utc)
    us = dt.microsecond
    if us == 0:
        frac = ""
    elif us % 1000 == 0:
        frac = ".%03d" % (us // 1000)
    else:
        frac = ".%06d" % us
    return dt.strftime("%Y-%m-%dT%H:%M:%S") + frac + "+00:00"


def compute_record_hash(row):
    """Recompute the record hash exactly as audit_log.rs does."""
    status = row.get("response_status")
    payload = "".join(
        [
            str(row["request_id"]),
            chrono_rfc3339(row["timestamp"]),
            row.get("requester_address") or "",
            row.get("action") or "",
            row.get("endpoint") or "",
            row.get("method") or "",
            row.get("ip_address") or "",
            str(0 if status is None else status),
            row.get("previous_hash") or "",
        ]
    )
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def verify_chain(rows):
    """Verify every record hash and every chain link. Returns (ok, detail)."""
    previous_hash = None
    for row in rows:
        row_id = row.get("id", "?")
        computed = compute_record_hash(row)
        if computed != row.get("record_hash"):
            return (
                False,
                "hash mismatch at id=%s: stored=%s computed=%s"
                % (row_id, row.get("record_hash"), computed),
            )
        if (row.get("previous_hash") or None) != previous_hash:
            return (
                False,
                "broken chain link at id=%s: stored previous_hash=%s, expected %s"
                % (row_id, row.get("previous_hash"), previous_hash),
            )
        previous_hash = row.get("record_hash")
    return True, "chain verified (%d records)" % len(rows)


def fetch_rows(database_url):
    if not database_url:
        print(
            "ERROR: no database URL. Fix: set DATABASE_URL or pass --database-url\n"
            "  e.g. DATABASE_URL=postgres://user:pass@localhost:5432/coordinator_db",
            file=sys.stderr,
        )
        sys.exit(1)
    try:
        result = subprocess.run(
            [
                "psql",
                database_url,
                "-X",
                "-A",
                "-q",
                "-c",
                "SELECT row_to_json(t)::text FROM (" + ROW_QUERY + ") t",
            ],
            capture_output=True,
            text=True,
            check=True,
        )
    except FileNotFoundError:
        print(
            "ERROR: psql not found. Fix: install PostgreSQL client tools "
            "(e.g. 'sudo apt-get install -y postgresql-client')",
            file=sys.stderr,
        )
        sys.exit(1)
    except subprocess.CalledProcessError as exc:
        print("ERROR: reading audit_logs failed: %s" % exc.stderr.strip(), file=sys.stderr)
        sys.exit(1)
    rows = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    return rows


def export_rows(rows, output_dir, retention_days, exported_at):
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    first_id = rows[0]["id"]
    last_id = rows[-1]["id"]
    stamp = exported_at.strftime("%Y%m%dT%H%M%SZ")
    base = "audit-%s-%s-%s" % (first_id, last_id, stamp)
    data_path = output_dir / (base + ".jsonl")
    manifest_path = output_dir / (base + ".manifest.json")

    # Exclusive create: an existing export is never rewritten or overwritten.
    try:
        with open(data_path, "x", encoding="utf-8") as handle:
            for row in rows:
                handle.write(json.dumps(row, sort_keys=True) + "\n")
    except FileExistsError:
        print(
            "ERROR: %s already exists — append-only exports are never overwritten; "
            "re-run later so a new timestamped file is created." % data_path,
            file=sys.stderr,
        )
        sys.exit(1)

    file_sha256 = hashlib.sha256(data_path.read_bytes()).hexdigest()
    manifest = {
        "exported_at": exported_at.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "source": "audit_logs",
        "record_count": len(rows),
        "first_id": first_id,
        "last_id": last_id,
        "chain_head_hash": rows[-1]["record_hash"],
        "integrity": "hash-chain-verified",
        "file": data_path.name,
        "file_sha256": file_sha256,
        "retention_days": retention_days,
        "retain_until": (exported_at + timedelta(days=retention_days)).strftime(
            "%Y-%m-%d"
        ),
    }
    with open(manifest_path, "x", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=True)
        handle.write("\n")

    os.chmod(data_path, 0o444)
    os.chmod(manifest_path, 0o444)
    return data_path, manifest_path, manifest


def self_test():
    failures = []

    def check(label, condition):
        if condition:
            print("  [ OK ] %s" % label)
        else:
            failures.append(label)
            print("  [FAIL] %s" % label)

    print("Running export_audit_worm.py self-test ...")

    hash_a = compute_record_hash(FIXTURE_A)
    check("fixture A reference hash", hash_a == FIXTURE_A_HASH)
    hash_b = compute_record_hash(FIXTURE_B)
    check("fixture B reference hash (chrono .100000 -> .100)", hash_b == FIXTURE_B_HASH)

    rows = [
        {"id": 1, "record_hash": hash_a, "previous_hash": None, **FIXTURE_A},
        {"id": 2, "record_hash": hash_b, "previous_hash": hash_a, **FIXTURE_B},
    ]
    ok, detail = verify_chain(rows)
    check("intact chain verifies: %s" % detail, ok)

    tampered = json.loads(json.dumps(rows))
    tampered[1]["action"] = "session.granted"
    ok, detail = verify_chain(tampered)
    check("tampered record rejected: %s" % detail, not ok)

    relinked = json.loads(json.dumps(rows))
    relinked[1]["previous_hash"] = "0" * 64
    relinked[1]["record_hash"] = compute_record_hash(relinked[1])
    ok, detail = verify_chain(relinked)
    check("broken link rejected: %s" % detail, not ok and "link" in detail)

    with tempfile.TemporaryDirectory() as tmp:
        exported_at = datetime(2026, 3, 4, tzinfo=timezone.utc)
        data_path, manifest_path, manifest = export_rows(
            rows, tmp, DEFAULT_RETENTION_DAYS, exported_at
        )
        check("export written: %s" % data_path.name, data_path.is_file())
        check(
            "manifest records chain head",
            manifest["chain_head_hash"] == hash_b,
        )
        check(
            "manifest records retention",
            manifest["retention_days"] == DEFAULT_RETENTION_DAYS
            and manifest["retain_until"] == "2027-03-04",
        )
        check("export file is read-only", not os.access(data_path, os.W_OK))
        wrote_again = False
        try:
            with open(data_path, "a", encoding="utf-8"):
                wrote_again = True
        except PermissionError:
            pass
        check("append-only file rejects writes", not wrote_again)
        duplicate_rejected = False
        try:
            with contextlib.redirect_stderr(io.StringIO()):
                export_rows(rows, tmp, DEFAULT_RETENTION_DAYS, exported_at)
        except SystemExit:
            duplicate_rejected = True
        check("duplicate export path refused (no overwrite)", duplicate_rejected)

    if failures:
        print("Self-test FAILED: %d check(s)" % len(failures))
        return 1
    print("Self-test passed (9 checks).")
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Verify and export audit_logs to append-only (WORM) storage."
    )
    parser.add_argument(
        "--database-url",
        default=os.environ.get("DATABASE_URL", ""),
        help="Postgres connection URL (default: $DATABASE_URL)",
    )
    parser.add_argument(
        "--output-dir",
        default="worm/audit-logs",
        help="append-only export directory (default: worm/audit-logs)",
    )
    parser.add_argument(
        "--retention-days",
        type=int,
        default=DEFAULT_RETENTION_DAYS,
        help="retention window recorded in the manifest (default: 365)",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run offline integrity checks without touching the database",
    )
    args = parser.parse_args()

    if args.self_test:
        sys.exit(self_test())

    rows = fetch_rows(args.database_url)
    if not rows:
        print("audit_logs is empty — nothing to export.")
        return

    ok, detail = verify_chain(rows)
    if not ok:
        print(
            "ERROR: audit log integrity check failed: %s\n"
            "  Refusing to export a tampered or corrupt chain. Investigate the\n"
            "  records above before exporting." % detail,
            file=sys.stderr,
        )
        sys.exit(1)

    exported_at = datetime.now(timezone.utc)
    data_path, manifest_path, manifest = export_rows(
        rows, args.output_dir, args.retention_days, exported_at
    )
    print("Integrity: %s" % detail)
    print("Exported %d records to %s" % (manifest["record_count"], data_path))
    print("Manifest: %s (retain until %s)" % (manifest_path, manifest["retain_until"]))


if __name__ == "__main__":
    main()

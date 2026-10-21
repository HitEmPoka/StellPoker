#!/usr/bin/env python3
"""Export the on-chain hand history buffer into an off-chain archive file (#684).

Paginates the poker-table contract's chunked read API
(`get_hand_history_chunk`) from the newest hand backwards and appends new
records to a JSONL file, one hand per line, sorted by hand_number ascending.
Re-runs are idempotent: hands already present in the output are skipped.

See docs/hand-history-archive.md for the pipeline, archive API, and the
retention policy.

Usage:
  ./scripts/export-hand-history-archive.py --contract-id C... --table-id 0 \
      --out archive/hands-table-0.jsonl

Requires the `stellar` CLI on PATH (or --stellar-bin), plus:
  RPC_URL / --rpc-url, STELLAR_NETWORK_PASSPHRASE / --network-passphrase
for a non-default network.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

DEFAULT_CHUNK = 16  # contract buffer size; chunk reads are capped by it


def stellar_invoke(args: list[str], stellar_bin: str) -> object:
    """Run `stellar contract invoke ...` and return parsed JSON."""
    cmd = [stellar_bin, "contract", "invoke", *args]
    try:
        out = subprocess.run(
            cmd, check=True, capture_output=True, text=True
        ).stdout
    except FileNotFoundError:
        sys.exit(f"error: '{stellar_bin}' not found on PATH (install the Stellar CLI)")
    except subprocess.CalledProcessError as exc:
        sys.exit(
            f"error: {' '.join(cmd)} failed ({exc.returncode})\n{exc.stderr.strip()}"
        )
    out = out.strip()
    if not out:
        return None
    return json.loads(out)


def invoke(
    contract_id: str,
    fn: str,
    fn_args: list[str],
    common: list[str],
    stellar_bin: str,
) -> object:
    return stellar_invoke(
        ["--id", contract_id, "--", fn, *fn_args, *common], stellar_bin
    )


def load_existing(path: Path) -> dict[tuple[str, int], dict]:
    """Index hands already exported, keyed by (table_id, hand_number)."""
    existing: dict[tuple[str, int], dict] = {}
    if not path.exists():
        return existing
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            hand = json.loads(line)
            key = (str(hand["table_id"]), int(hand["hand_number"]))
            existing[key] = hand
        except (json.JSONDecodeError, KeyError, TypeError, ValueError):
            # Tolerate a partial trailing line from an interrupted run.
            continue
    return existing


def export_table(
    contract_id: str,
    table_id: int,
    chunk: int,
    path: Path,
    common: list[str],
    stellar_bin: str,
) -> int:
    meta = invoke(
        contract_id,
        "get_hand_history_meta",
        ["--table_id", str(table_id)],
        common,
        stellar_bin,
    )
    total_archived = int((meta or {}).get("total_archived", 0))
    if total_archived == 0:
        print(f"table {table_id}: empty buffer, nothing to export")
        return 0

    existing = load_existing(path)
    new_hands: list[dict] = []
    offset = 0  # 0 = newest; walk backwards until we cover total_archived
    while offset < total_archived:
        page = invoke(
            contract_id,
            "get_hand_history_chunk",
            [
                "--table_id", str(table_id),
                "--offset", str(offset),
                "--limit", str(chunk),
            ],
            common,
            stellar_bin,
        )
        rows = page or []
        if not rows:
            break
        for row in rows:
            row = dict(row)
            row["table_id"] = table_id  # normalize key for the archive index
            hand_number = int(row.get("hand_number", -1))
            if hand_number < 0:
                continue
            key = (str(table_id), hand_number)
            if key not in existing and not any(
                h["hand_number"] == hand_number for h in new_hands
            ):
                new_hands.append(row)
        offset += len(rows)
        if len(rows) < chunk:
            break

    if new_hands:
        new_hands.sort(key=lambda h: int(h["hand_number"]))
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("a") as fh:
            for hand in new_hands:
                fh.write(json.dumps(hand, separators=(",", ":"), sort_keys=True))
                fh.write("\n")
    print(
        f"table {table_id}: total_archived={total_archived} "
        f"already={len(existing)} appended={len(new_hands)} -> {path}"
    )
    return len(new_hands)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--contract-id", required=True, help="poker-table contract id")
    parser.add_argument(
        "--table-id", type=int, required=True, help="table to export"
    )
    parser.add_argument(
        "--out", required=True, help="output JSONL file (appended to)"
    )
    parser.add_argument(
        "--chunk",
        type=int,
        default=DEFAULT_CHUNK,
        help="records per chunk read (default: contract buffer size 16)",
    )
    parser.add_argument(
        "--rpc-url",
        default=os.environ.get("RPC_URL", "https://soroban-testnet.stellar.org"),
    )
    parser.add_argument(
        "--network-passphrase",
        default=os.environ.get(
            "STELLAR_NETWORK_PASSPHRASE", "Test SDF Network ; September 2015"
        ),
    )
    parser.add_argument(
        "--stellar-bin", default=os.environ.get("STELLAR_BIN", "stellar")
    )
    args = parser.parse_args()

    common = [
        "--rpc-url", args.rpc_url,
        "--network-passphrase", args.network_passphrase,
    ]
    export_table(
        args.contract_id,
        args.table_id,
        args.chunk,
        Path(args.out),
        common,
        args.stellar_bin,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

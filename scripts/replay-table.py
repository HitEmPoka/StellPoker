#!/usr/bin/env python3
"""Seed a hand replayer from `export_table_state` output (#570).

Usage:
    stellar contract invoke ... --fn export_table_state --arg table_id \
        | python3 scripts/replay-table.py
    python3 scripts/replay-table.py export.json

Reads the versioned TableExportV1 snapshot (rejects unknown `version`)
and prints the initial replay state: phase, button, stacks/seats,
pot and config. The replayer then applies the on-chain action history
(player_action / reveal_board / submit_showdown events) on top.
"""

import json
import sys

SUPPORTED_VERSION = 1


def load_export(path=None):
    raw = open(path).read() if path else sys.stdin.read()
    data = json.loads(raw)
    # `stellar contract invoke --output json` wraps the return value in a
    # list; unwrap single-element lists.
    if isinstance(data, list) and len(data) == 1:
        data = data[0]
    return data


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else None
    export = load_export(path)

    version = export.get("version")
    if version != SUPPORTED_VERSION:
        sys.exit(
            f"unsupported table export version: {version!r} "
            f"(supports {SUPPORTED_VERSION})"
        )

    print(f"table={export['table_id']} phase={export['phase']} "
          f"button={export['dealer_seat']} hand={export['hand_number']} "
          f"pot={export['pot']}")
    for seat in export.get("seats", []):
        print(f"  seat={seat['seat_index']} stack={seat['stack']} "
              f"bet={seat['bet_this_round']} committed={seat['committed']} "
              f"folded={seat['folded']} all_in={seat['all_in']}")
    print(f"board={export.get('board_cards', [])}")
    print(f"config={json.dumps(export.get('config', {}))}")


if __name__ == "__main__":
    main()

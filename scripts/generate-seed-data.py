#!/usr/bin/env python3
"""Generate realistic development/load-testing seed data for Stellar Poker.

Produces:
  - SQL INSERTs for `game_tables` and `player_stats` (coordinator DB schema,
    see services/coordinator/migrations/20240101000000_initial_schema.up.sql)
  - A JSON fixture of sample hand histories, in the shape the frontend's
    replay viewer (app/src/components/ReplayViewer.tsx) expects: a list of
    hands, each a list of board/hole card values 0-51 (see app/src/lib/cards.ts
    for the encoding: value = suit_index * 13 + rank_index).

Deterministic when --seed is given, so generated fixtures are reproducible
across runs and CI.

Usage:
    python3 scripts/generate-seed-data.py \\
        --tables 10 --players 40 --hands 50 --seed 42 \\
        --sql-out /tmp/seed.sql --hands-out /tmp/hand-history.json

    # Apply the generated SQL to a local coordinator DB:
    psql "$DATABASE_URL" -f /tmp/seed.sql

Run with --help for the full list of configurable parameters.
"""

import argparse
import json
import random
import string
import sys
from dataclasses import dataclass, field

SUITS = ["clubs", "diamonds", "hearts", "spades"]
RANKS = ["2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"]
PHASES = ["waiting", "dealing", "reveal_flop", "reveal_turn", "reveal_river", "showdown", "complete"]

AI_NAME_PREFIXES = [
    "Bluff", "Grind", "River", "Nit", "Shark", "Whale", "Rock", "Maniac",
    "Fish", "Ace", "Chip", "Pot", "Fold", "Raise", "Check",
]
AI_NAME_SUFFIXES = ["Bot", "AI", "9000", "Prime", "Zero", "X", "Jr"]


def card_value(rank_index: int, suit_index: int) -> int:
    """Encode a card the same way app/src/lib/cards.ts does: suit*13 + rank."""
    return suit_index * 13 + rank_index


def random_stellar_address(rng: random.Random) -> str:
    """A syntactically plausible (not cryptographically valid) G... address,
    good enough for local dev/load-testing where signatures aren't checked."""
    body = "".join(rng.choices(string.ascii_uppercase + "234567", k=55))
    return f"G{body}"


def random_ai_name(rng: random.Random) -> str:
    return f"{rng.choice(AI_NAME_PREFIXES)}{rng.choice(AI_NAME_SUFFIXES)}{rng.randint(1, 999)}"


@dataclass
class Player:
    address: str
    name: str
    hands_played: int
    hands_won: int
    total_winnings: int  # stroops


@dataclass
class Table:
    table_id: int
    phase: str
    deck_root: str | None
    player_order: list[str]
    proof_nonce: int


@dataclass
class Hand:
    hand_id: str
    table_id: int
    board_cards: list[int]
    players: list[dict] = field(default_factory=list)
    winner_address: str = ""
    pot: int = 0


def gen_deck(rng: random.Random) -> list[int]:
    deck = [card_value(r, s) for s in range(4) for r in range(13)]
    rng.shuffle(deck)
    return deck


def gen_players(rng: random.Random, count: int) -> list[Player]:
    players = []
    for _ in range(count):
        hands_played = rng.randint(0, 500)
        hands_won = rng.randint(0, hands_played)
        players.append(
            Player(
                address=random_stellar_address(rng),
                name=random_ai_name(rng),
                hands_played=hands_played,
                hands_won=hands_won,
                total_winnings=rng.randint(-50_000_000, 2_000_000_000),
            )
        )
    return players


def gen_tables(rng: random.Random, count: int, players: list[Player], seats: int) -> list[Table]:
    tables = []
    for table_id in range(1, count + 1):
        phase = rng.choice(PHASES)
        seated = rng.sample(players, k=min(seats, len(players)))
        has_deck = phase != "waiting"
        tables.append(
            Table(
                table_id=table_id,
                phase=phase,
                deck_root=("0x" + "".join(rng.choices("0123456789abcdef", k=64))) if has_deck else None,
                player_order=[p.address for p in seated],
                proof_nonce=rng.randint(0, 50),
            )
        )
    return tables


def gen_hands(rng: random.Random, count: int, tables: list[Table], players: list[Player]) -> list[Hand]:
    hands = []
    for i in range(count):
        table = rng.choice(tables)
        deck = gen_deck(rng)
        board = deck[:5]
        seated = [p for p in players if p.address in table.player_order] or rng.sample(players, k=min(2, len(players)))
        hole_cursor = 5
        hand_players = []
        for p in seated:
            hole = deck[hole_cursor:hole_cursor + 2]
            hole_cursor += 2
            hand_players.append({"address": p.address, "cards": hole})
        winner = rng.choice(seated) if seated else None
        hands.append(
            Hand(
                hand_id=f"hand-{table.table_id}-{i + 1}",
                table_id=table.table_id,
                board_cards=board,
                players=hand_players,
                winner_address=winner.address if winner else "",
                pot=rng.randint(1_000_000, 500_000_000),
            )
        )
    return hands


def sql_literal(value) -> str:
    if value is None:
        return "NULL"
    if isinstance(value, bool):
        return "TRUE" if value else "FALSE"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, list):
        inner = ",".join(sql_literal(v) for v in value)
        return f"ARRAY[{inner}]" if value else "'{}'"
    escaped = str(value).replace("'", "''")
    return f"'{escaped}'"


def render_sql(tables: list[Table], players: list[Player]) -> str:
    lines = [
        "-- Generated by scripts/generate-seed-data.py — do not edit by hand.",
        "-- Regenerate with: python3 scripts/generate-seed-data.py --sql-out <file>",
        "BEGIN;",
        "",
        "-- Tables",
    ]
    for t in tables:
        lines.append(
            "INSERT INTO game_tables (table_id, phase, deck_root, player_order, proof_nonce) "
            f"VALUES ({sql_literal(t.table_id)}, {sql_literal(t.phase)}, {sql_literal(t.deck_root)}, "
            f"{sql_literal(t.player_order)}, {sql_literal(t.proof_nonce)}) "
            "ON CONFLICT (table_id) DO UPDATE SET phase = EXCLUDED.phase, "
            "deck_root = EXCLUDED.deck_root, player_order = EXCLUDED.player_order, "
            "proof_nonce = EXCLUDED.proof_nonce;"
        )
    lines.append("")
    lines.append("-- Player stats")
    for p in players:
        lines.append(
            "INSERT INTO player_stats (address, hands_played, hands_won, total_winnings) "
            f"VALUES ({sql_literal(p.address)}, {sql_literal(p.hands_played)}, "
            f"{sql_literal(p.hands_won)}, {sql_literal(p.total_winnings)}) "
            "ON CONFLICT (address) DO UPDATE SET hands_played = EXCLUDED.hands_played, "
            "hands_won = EXCLUDED.hands_won, total_winnings = EXCLUDED.total_winnings;"
        )
    lines.append("")
    lines.append("COMMIT;")
    lines.append("")
    return "\n".join(lines)


def render_hands_json(hands: list[Hand], players: list[Player]) -> str:
    payload = {
        "players": [
            {
                "address": p.address,
                "name": p.name,
                "handsPlayed": p.hands_played,
                "handsWon": p.hands_won,
                "totalWinnings": p.total_winnings,
            }
            for p in players
        ],
        "hands": [
            {
                "handId": h.hand_id,
                "tableId": h.table_id,
                "boardCards": h.board_cards,
                "players": h.players,
                "winnerAddress": h.winner_address,
                "pot": h.pot,
            }
            for h in hands
        ],
    }
    return json.dumps(payload, indent=2) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tables", type=int, default=10, help="Number of tables to generate (default: 10)")
    parser.add_argument("--players", type=int, default=40, help="Number of AI players to generate (default: 40)")
    parser.add_argument("--seats", type=int, default=6, help="Max seats per table (default: 6)")
    parser.add_argument("--hands", type=int, default=25, help="Number of sample hand histories to generate (default: 25)")
    parser.add_argument("--seed", type=int, default=None, help="RNG seed for reproducible output (default: unseeded/random)")
    parser.add_argument("--sql-out", default="-", help="Path to write the SQL seed file to, or '-' for stdout (default: stdout)")
    parser.add_argument("--hands-out", default=None, help="Path to write the hand-history JSON fixture to (default: not written)")
    args = parser.parse_args()

    if args.tables < 1 or args.players < 1 or args.hands < 0 or args.seats < 1:
        parser.error("--tables, --players, and --seats must be >= 1, --hands must be >= 0")

    rng = random.Random(args.seed)

    players = gen_players(rng, args.players)
    tables = gen_tables(rng, args.tables, players, args.seats)
    hands = gen_hands(rng, args.hands, tables, players) if args.hands else []

    sql = render_sql(tables, players)
    if args.sql_out == "-":
        sys.stdout.write(sql)
    else:
        with open(args.sql_out, "w") as f:
            f.write(sql)
        print(f"Wrote {len(tables)} tables and {len(players)} players to {args.sql_out}", file=sys.stderr)

    if args.hands_out:
        with open(args.hands_out, "w") as f:
            f.write(render_hands_json(hands, players))
        print(f"Wrote {len(hands)} hand histories to {args.hands_out}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

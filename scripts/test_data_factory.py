"""Deterministic fixtures for endurance and integration tests."""

from dataclasses import dataclass
import random


@dataclass(frozen=True)
class PlayerFixture:
    address: str
    buy_in: int


@dataclass(frozen=True)
class TableFixture:
    table_id: int
    players: tuple[PlayerFixture, ...]


class PlayerFactory:
    """Generate stable, unique player identities for a test run."""

    def __init__(self, rng: random.Random):
        self._rng = rng
        self._next_id = 1

    def create(self) -> PlayerFixture:
        player_id = self._next_id
        self._next_id += 1
        suffix = self._rng.randrange(10**12)
        address = f"PLAYER_{player_id:06d}_{suffix:012d}"
        return PlayerFixture(address=address, buy_in=self._rng.choice((500, 1000, 2500, 5000)))


class TableFactory:
    """Create realistic solo and multiplayer table distributions."""

    def __init__(self, rng: random.Random, player_factory: PlayerFactory):
        self._rng = rng
        self._players = player_factory

    def create(self) -> TableFixture:
        # Small tables are common, while six-player tables exercise the widest
        # request payloads without making the soak test artificially expensive.
        player_count = self._rng.choices((1, 2, 3, 4, 5, 6), weights=(20, 30, 20, 15, 10, 5), k=1)[0]
        players = tuple(self._players.create() for _ in range(player_count))
        return TableFixture(table_id=self._rng.randint(1, 999_999_999), players=players)

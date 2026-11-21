import random
import unittest

from scripts.test_data_factory import PlayerFactory, TableFactory


class TestDataFactory(unittest.TestCase):
    def test_seeded_tables_are_reproducible(self):
        def sample(seed):
            factory = TableFactory(random.Random(seed), PlayerFactory(random.Random(seed)))
            return [factory.create() for _ in range(5)]

        self.assertEqual(sample(42), sample(42))

    def test_factory_generates_solo_and_multiplayer_tables(self):
        factory = TableFactory(random.Random(7), PlayerFactory(random.Random(7)))
        counts = [len(factory.create().players) for _ in range(500)]
        self.assertIn(1, counts)
        self.assertTrue(any(count > 1 for count in counts))
        self.assertTrue(all(1 <= count <= 6 for count in counts))

    def test_players_have_unique_addresses_and_valid_buy_ins(self):
        players = PlayerFactory(random.Random(1))
        fixtures = [players.create() for _ in range(100)]
        self.assertEqual(len({player.address for player in fixtures}), len(fixtures))
        self.assertTrue(all(player.buy_in in {500, 1000, 2500, 5000} for player in fixtures))

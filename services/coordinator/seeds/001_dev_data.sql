-- Development seed data for the coordinator database.
-- Provides three sample tables and player stats for local testing.
-- Run with: psql "$DATABASE_URL" -f services/coordinator/seeds/001_dev_data.sql

BEGIN;

-- Sample tables in various phases
INSERT INTO game_tables (table_id, phase, deck_root, player_order, proof_nonce)
VALUES
    (1, 'waiting',  NULL,                                                                     0),
    (2, 'dealing',  '0xabc123def456abc123def456abc123def456abc123def456abc123def456abc123', 3),
    (3, 'complete', '0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef', 12)
ON CONFLICT (table_id) DO NOTHING;

-- Sample MPC sessions linked to table 2
INSERT INTO mpc_sessions (table_id, session_type, status, tx_hash)
VALUES
    (2, 'deal',    'complete', '0x1111111111111111111111111111111111111111111111111111111111111111'),
    (2, 'reveal',  'running',  NULL)
ON CONFLICT DO NOTHING;

-- Sample player stats
INSERT INTO player_stats (address, hands_played, hands_won, total_winnings)
VALUES
    ('GDQOE23CFSUMSVQK4Y5JHPPYK73VYCNHZHA7ENKCV37P6SUEO6XQBKPP', 42, 18, 850000000),
    ('GDRXE2BQUC3AZNPVFSCEZ76NJ3WWL25FYFK6RIGPACGIU3WGBMUQATKJH', 15, 6,  310000000),
    ('GBVG2QOHHFBVHAEGNF4XRUCAPAGWDROONM2LC4BK6MJXI5EME4OCQBYI',  7,  2,   90000000)
ON CONFLICT (address) DO UPDATE SET
    hands_played   = EXCLUDED.hands_played,
    hands_won      = EXCLUDED.hands_won,
    total_winnings = EXCLUDED.total_winnings;

COMMIT;

-- Rollback: drop all objects created by 20240101000000_initial_schema.up.sql

DROP TRIGGER IF EXISTS trg_player_stats_updated_at  ON player_stats;
DROP TRIGGER IF EXISTS trg_auth_nonces_updated_at   ON auth_nonces;
DROP TRIGGER IF EXISTS trg_game_tables_updated_at   ON game_tables;

DROP FUNCTION IF EXISTS set_updated_at();

DROP TABLE IF EXISTS mpc_sessions;
DROP TABLE IF EXISTS game_tables;
DROP TABLE IF EXISTS auth_nonces;
DROP TABLE IF EXISTS player_stats;

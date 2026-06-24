-- Initial coordinator schema
-- Mirrors the in-memory structures in coordinator/src/main.rs so the service
-- can persist state across restarts when DATABASE_URL is provided.

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- -------------------------------------------------------------------------
-- game_tables
-- Represents an active or completed poker table (TableSession in Rust).
-- -------------------------------------------------------------------------
CREATE TABLE game_tables (
    id                       BIGSERIAL    PRIMARY KEY,
    table_id                 INTEGER      UNIQUE NOT NULL,
    phase                    TEXT         NOT NULL DEFAULT 'waiting'
                                          CHECK (phase IN ('waiting', 'dealing', 'reveal_flop', 'reveal_turn', 'reveal_river', 'showdown', 'complete')),
    deck_root                TEXT,
    proof_nonce              BIGINT       NOT NULL DEFAULT 0,
    -- Ordered seat list; index = seat position.
    player_order             TEXT[]       NOT NULL DEFAULT '{}',
    -- Per-player hand commitments aligned with player_order.
    hand_commitments         TEXT[]       NOT NULL DEFAULT '{}',
    -- Deck indices of cards that have been dealt.
    dealt_indices            INTEGER[]    NOT NULL DEFAULT '{}',
    -- Deck indices of revealed board cards.
    board_indices            INTEGER[]    NOT NULL DEFAULT '{}',
    deal_session_id          TEXT,
    deal_tx_hash             TEXT,
    showdown_tx_hash         TEXT,
    showdown_winner          TEXT,
    showdown_winning_amount  BIGINT,
    created_at               TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at               TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_game_tables_phase ON game_tables (phase);

-- -------------------------------------------------------------------------
-- mpc_sessions
-- Tracks each co-Noir proof generation attempt (deal / reveal / showdown).
-- -------------------------------------------------------------------------
CREATE TABLE mpc_sessions (
    id           UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    table_id     INTEGER      NOT NULL REFERENCES game_tables (table_id) ON DELETE CASCADE,
    session_type TEXT         NOT NULL CHECK (session_type IN ('deal', 'reveal', 'showdown')),
    -- For reveal sessions the phase identifies which street (flop/turn/river).
    phase        TEXT,
    status       TEXT         NOT NULL DEFAULT 'pending'
                              CHECK (status IN ('pending', 'running', 'complete', 'failed', 'cancelled')),
    tx_hash      TEXT,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX idx_mpc_sessions_table_id ON mpc_sessions (table_id);
CREATE INDEX idx_mpc_sessions_status   ON mpc_sessions (status);

-- -------------------------------------------------------------------------
-- auth_nonces
-- Monotonic per-address nonce used to prevent replay attacks.
-- -------------------------------------------------------------------------
CREATE TABLE auth_nonces (
    address     TEXT         PRIMARY KEY,
    last_nonce  BIGINT       NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- -------------------------------------------------------------------------
-- player_stats
-- Cumulative game statistics indexed from Horizon (stats.rs indexer).
-- -------------------------------------------------------------------------
CREATE TABLE player_stats (
    address        TEXT         PRIMARY KEY,
    hands_played   INTEGER      NOT NULL DEFAULT 0,
    hands_won      INTEGER      NOT NULL DEFAULT 0,
    total_winnings BIGINT       NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- -------------------------------------------------------------------------
-- updated_at trigger
-- -------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_game_tables_updated_at
    BEFORE UPDATE ON game_tables
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_auth_nonces_updated_at
    BEFORE UPDATE ON auth_nonces
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER trg_player_stats_updated_at
    BEFORE UPDATE ON player_stats
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

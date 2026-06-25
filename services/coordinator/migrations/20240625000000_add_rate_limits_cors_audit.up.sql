-- Migration for issues #267, #261, #265
-- Adds rate limit configuration, CORS configuration, and audit logging tables

-- -------------------------------------------------------------------------
-- rate_limit_configs
-- Configurable rate limits per endpoint, per wallet, or globally
-- -------------------------------------------------------------------------
CREATE TABLE rate_limit_configs (
    id                BIGSERIAL    PRIMARY KEY,
    config_type       TEXT         NOT NULL CHECK (config_type IN ('global', 'endpoint', 'wallet', 'endpoint_wallet')),
    endpoint          TEXT,        -- NULL for global and wallet-only limits
    wallet_address    TEXT,        -- NULL for global and endpoint-only limits
    max_requests      INTEGER      NOT NULL,
    window_seconds    INTEGER      NOT NULL DEFAULT 60,
    enabled           BOOLEAN      NOT NULL DEFAULT true,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE NULLS NOT DISTINCT (config_type, endpoint, wallet_address)
);

CREATE INDEX idx_rate_limit_configs_type ON rate_limit_configs (config_type);
CREATE INDEX idx_rate_limit_configs_endpoint ON rate_limit_configs (endpoint);
CREATE INDEX idx_rate_limit_configs_wallet ON rate_limit_configs (wallet_address);

CREATE TRIGGER trg_rate_limit_configs_updated_at
    BEFORE UPDATE ON rate_limit_configs
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- -------------------------------------------------------------------------
-- cors_configs
-- Configurable CORS allowed origins for production
-- -------------------------------------------------------------------------
CREATE TABLE cors_configs (
    id           BIGSERIAL    PRIMARY KEY,
    origin       TEXT         NOT NULL UNIQUE,
    enabled      BOOLEAN      NOT NULL DEFAULT true,
    description  TEXT,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_cors_configs_enabled ON cors_configs (enabled);

CREATE TRIGGER trg_cors_configs_updated_at
    BEFORE UPDATE ON cors_configs
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- -------------------------------------------------------------------------
-- audit_logs
-- Tamper-evident append-only audit log for compliance
-- -------------------------------------------------------------------------
CREATE TABLE audit_logs (
    id                BIGSERIAL    PRIMARY KEY,
    request_id        UUID         NOT NULL,
    timestamp         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    requester_address TEXT,
    action            TEXT         NOT NULL,
    endpoint          TEXT         NOT NULL,
    method            TEXT         NOT NULL,
    ip_address        TEXT,
    response_status   INTEGER,
    error_message     TEXT,
    table_id          INTEGER,
    session_id        TEXT,
    -- Hash chain for tamper detection
    previous_hash     TEXT,
    record_hash       TEXT         NOT NULL
);

CREATE INDEX idx_audit_logs_timestamp ON audit_logs (timestamp DESC);
CREATE INDEX idx_audit_logs_requester ON audit_logs (requester_address);
CREATE INDEX idx_audit_logs_action ON audit_logs (action);
CREATE INDEX idx_audit_logs_table_id ON audit_logs (table_id);

-- Prevent updates and deletes to maintain append-only property
CREATE RULE audit_logs_no_update AS ON UPDATE TO audit_logs DO INSTEAD NOTHING;
CREATE RULE audit_logs_no_delete AS ON DELETE TO audit_logs DO INSTEAD NOTHING;

-- -------------------------------------------------------------------------
-- session_migrations
-- Tracks coordinator instance migrations for session handoff
-- -------------------------------------------------------------------------
CREATE TABLE session_migrations (
    id                    BIGSERIAL    PRIMARY KEY,
    session_id            TEXT         NOT NULL,
    table_id              INTEGER      NOT NULL,
    from_instance_id      TEXT         NOT NULL,
    to_instance_id        TEXT         NOT NULL,
    migration_status      TEXT         NOT NULL DEFAULT 'initiated' CHECK (migration_status IN ('initiated', 'transferring', 'complete', 'failed')),
    state_snapshot        JSONB,
    mpc_connections       JSONB,
    pending_actions       JSONB,
    initiated_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    completed_at          TIMESTAMPTZ,
    error_message         TEXT
);

CREATE INDEX idx_session_migrations_session_id ON session_migrations (session_id);
CREATE INDEX idx_session_migrations_status ON session_migrations (migration_status);
CREATE INDEX idx_session_migrations_instance ON session_migrations (from_instance_id, to_instance_id);

-- Insert default global rate limits
INSERT INTO rate_limit_configs (config_type, max_requests, window_seconds)
VALUES ('global', 100, 60);

-- Insert default CORS config (permissive for dev, should be updated in production)
INSERT INTO cors_configs (origin, description)
VALUES 
    ('*', 'Development permissive origin - update for production'),
    ('http://localhost:3000', 'Local development frontend'),
    ('http://localhost:8080', 'Local development coordinator');

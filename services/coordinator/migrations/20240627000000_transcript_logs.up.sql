-- -------------------------------------------------------------------------
-- transcript_logs
-- Issue #578: tamper-evident MPC protocol transcript hashing, chained per
-- session_id (one chain per hand, across its Deal -> Reveal -> Showdown
-- phases). Mirrors audit_logs' hash-chain shape; see transcript_log.rs.
-- -------------------------------------------------------------------------
CREATE TABLE transcript_logs (
    id                   BIGSERIAL    PRIMARY KEY,
    session_id           TEXT         NOT NULL,
    phase                TEXT         NOT NULL,
    public_inputs_digest TEXT         NOT NULL,
    proof_hash           TEXT,
    recorded_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    -- Hash chain for tamper detection, scoped per session_id
    previous_hash        TEXT,
    record_hash          TEXT         NOT NULL
);

CREATE INDEX idx_transcript_logs_session_id ON transcript_logs (session_id, id);
CREATE INDEX idx_transcript_logs_recorded_at ON transcript_logs (recorded_at DESC);

-- Prevent updates and deletes to maintain append-only property
CREATE RULE transcript_logs_no_update AS ON UPDATE TO transcript_logs DO INSTEAD NOTHING;
CREATE RULE transcript_logs_no_delete AS ON DELETE TO transcript_logs DO INSTEAD NOTHING;

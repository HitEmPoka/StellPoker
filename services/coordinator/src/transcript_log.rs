//! Tamper-evident MPC protocol transcript hashing for post-hoc audit
//!
//! Issue #578: auditability without revealing secrets — hash each MPC
//! session's phases into a tamper-evident hash chain, scoped per session_id
//! (the same session_id `generate_proof_from_share_sets` in `mpc.rs` already
//! threads across a hand's Deal → Reveal → Showdown phases).
//!
//! Mirrors `audit_log.rs`'s hash-chain pattern exactly, but the chain here
//! is scoped per `session_id` (one chain per hand) rather than one global
//! chain, and the hashed fields are deliberately limited to public-safe MPC
//! metadata — circuit name, a digest of the public inputs, and the proof
//! hash — never secret shares or witnesses.

use sha2::{Digest, Sha256};
use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TranscriptLog {
    pub id: i64,
    pub session_id: String,
    pub phase: String,
    pub public_inputs_digest: String,
    pub proof_hash: Option<String>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub previous_hash: Option<String>,
    pub record_hash: String,
}

/// Digest public inputs without ever hashing secret-shared data — callers
/// must pass only values already safe to reveal (e.g. `MpcProofResult::
/// public_inputs` from `mpc.rs`), never raw shares or witnesses.
pub fn digest_public_inputs(public_inputs: &[String]) -> String {
    let mut hasher = Sha256::new();
    for input in public_inputs {
        hasher.update(input.as_bytes());
        hasher.update(b"|");
    }
    format!("{:x}", hasher.finalize())
}

fn compute_record_hash(
    session_id: &str,
    phase: &str,
    public_inputs_digest: &str,
    proof_hash: Option<&str>,
    recorded_at: &chrono::DateTime<chrono::Utc>,
    previous_hash: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update(phase.as_bytes());
    hasher.update(public_inputs_digest.as_bytes());
    hasher.update(proof_hash.unwrap_or("").as_bytes());
    hasher.update(recorded_at.to_rfc3339().as_bytes());
    hasher.update(previous_hash.unwrap_or("").as_bytes());

    format!("{:x}", hasher.finalize())
}

/// Get the hash of the most recent transcript entry for this session,
/// so the chain is scoped per session_id rather than global.
async fn get_latest_hash_for_session(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let result: Option<(String,)> = sqlx::query_as(
        "SELECT record_hash FROM transcript_logs WHERE session_id = $1 ORDER BY id DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(result.map(|r| r.0))
}

/// Record one MPC phase's transcript hash, chained to the previous phase
/// recorded for the same `session_id`. Call once per phase (e.g. after
/// `generate_proof_from_share_sets` completes a Deal, Reveal, or Showdown
/// phase), passing only public-safe data.
pub async fn record_phase_transcript(
    pool: &PgPool,
    session_id: &str,
    phase: &str,
    public_inputs: &[String],
    proof_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    let recorded_at = chrono::Utc::now();
    let public_inputs_digest = digest_public_inputs(public_inputs);
    let previous_hash = get_latest_hash_for_session(pool, session_id).await?;

    let record_hash = compute_record_hash(
        session_id,
        phase,
        &public_inputs_digest,
        proof_hash,
        &recorded_at,
        previous_hash.as_deref(),
    );

    sqlx::query(
        "INSERT INTO transcript_logs
         (session_id, phase, public_inputs_digest, proof_hash, recorded_at, previous_hash, record_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(session_id)
    .bind(phase)
    .bind(&public_inputs_digest)
    .bind(proof_hash)
    .bind(recorded_at)
    .bind(previous_hash)
    .bind(record_hash)
    .execute(pool)
    .await?;

    Ok(())
}

/// Verify the integrity of one session's transcript hash chain.
pub async fn verify_transcript_chain(
    pool: &PgPool,
    session_id: &str,
) -> Result<bool, sqlx::Error> {
    let logs = sqlx::query_as::<_, TranscriptLog>(
        "SELECT id, session_id, phase, public_inputs_digest, proof_hash, recorded_at,
         previous_hash, record_hash
         FROM transcript_logs
         WHERE session_id = $1
         ORDER BY id ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    let mut previous_hash: Option<String> = None;

    for log in logs {
        let computed_hash = compute_record_hash(
            &log.session_id,
            &log.phase,
            &log.public_inputs_digest,
            log.proof_hash.as_deref(),
            &log.recorded_at,
            previous_hash.as_deref(),
        );

        if computed_hash != log.record_hash {
            tracing::error!(
                "Transcript chain integrity violation: session_id={}, log id={}, expected hash={}, actual={}",
                log.session_id,
                log.id,
                computed_hash,
                log.record_hash
            );
            return Ok(false);
        }

        if log.previous_hash != previous_hash {
            tracing::error!(
                "Transcript chain link broken: session_id={}, log id={}, expected previous={:?}, actual={:?}",
                log.session_id,
                log.id,
                previous_hash,
                log.previous_hash
            );
            return Ok(false);
        }

        previous_hash = Some(log.record_hash);
    }

    Ok(true)
}

/// List every distinct session_id with at least one transcript entry, for
/// the verification tool to iterate when no specific session is given.
pub async fn list_session_ids(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT session_id FROM transcript_logs ORDER BY session_id ASC")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

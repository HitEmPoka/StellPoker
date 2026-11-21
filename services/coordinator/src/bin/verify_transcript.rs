//! Standalone verification tool for MPC protocol transcript hash chains
//! (Issue #578).
//!
//! `src/bin/*.rs` files are separate crate roots with no access to
//! `main.rs`'s modules, so `db.rs` and `transcript_log.rs` are re-included
//! here by path rather than needing a `lib.rs` for the whole crate.
//!
//! Usage:
//!   DATABASE_URL=... cargo run --bin verify_transcript [session_id]
//!
//! With no `session_id`, verifies every session that has transcript
//! entries and reports a pass/fail summary; exits non-zero if any fail.

#[path = "../db.rs"]
mod db;
#[path = "../transcript_log.rs"]
mod transcript_log;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set to verify the transcript chain");

    let pool = db::connect(&database_url)
        .await
        .expect("failed to connect to the coordinator database");

    let args: Vec<String> = std::env::args().collect();
    let session_ids: Vec<String> = if let Some(id) = args.get(1) {
        vec![id.clone()]
    } else {
        transcript_log::list_session_ids(&pool)
            .await
            .expect("failed to list sessions with transcript entries")
    };

    if session_ids.is_empty() {
        println!("No transcript sessions found.");
        return;
    }

    let mut all_ok = true;
    for session_id in &session_ids {
        match transcript_log::verify_transcript_chain(&pool, session_id).await {
            Ok(true) => println!("PASS  {session_id}"),
            Ok(false) => {
                println!("FAIL  {session_id}  (chain integrity violation — see coordinator logs)");
                all_ok = false;
            }
            Err(e) => {
                println!("ERROR {session_id}  ({e})");
                all_ok = false;
            }
        }
    }

    println!(
        "\n{} session(s) checked, {}",
        session_ids.len(),
        if all_ok { "all passed" } else { "failures found" }
    );

    if !all_ok {
        std::process::exit(1);
    }
}

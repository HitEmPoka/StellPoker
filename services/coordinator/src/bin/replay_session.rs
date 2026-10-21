//! Print a redacted MPC session timeline for support investigations.
//!
//! Usage: `DATABASE_URL=... cargo run --bin replay_session -- SESSION_ID`
//! The output contains hashes and phase metadata only; secrets are never read.

#[path = "../db.rs"]
mod db;
#[path = "../session_replay.rs"]
mod session_replay;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set to replay a coordinator session");
    let session_id = std::env::args()
        .nth(1)
        .expect("usage: replay_session SESSION_ID");
    let pool = db::connect(&database_url)
        .await
        .expect("failed to connect to the coordinator database");
    let events = session_replay::replay_redacted(&pool, &session_id)
        .await
        .expect("failed to load transcript timeline");
    println!("{}", serde_json::to_string_pretty(&events).expect("serialize replay"));
}

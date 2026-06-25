//! Stellar Poker MPC Node
//!
//! Each node is a participant in the REP3 MPC protocol via TACEO's co-noir.
//! It holds secret shares and participates in collaborative proof generation.
//!
//! Lifecycle:
//! 1. Coordinator asks each node to prepare its own share bundle (/table/:id/prepare-*)
//! 2. Coordinator asks each node to dispatch its bundle to peers (/session/:id/shares)
//! 3. Coordinator triggers proof gen via POST /session/:id/generate
//! 4. Node merges all source fragments, then runs co-noir witness/proof subprocesses
//! 5. Coordinator polls GET /session/:id/status and retrieves proof via GET /session/:id/proof
//!
//! co-noir handles peer-to-peer MPC communication internally via TCP (ports 10000-10002).

use axum::{
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

mod api;
mod private_table;
mod session;

use private_table::PrivateTableState;
use session::MpcSessionState;

#[derive(Clone)]
pub struct NodeState {
    pub node_id: u32,
    pub sessions: Arc<RwLock<HashMap<String, Arc<RwLock<MpcSessionState>>>>>,
    pub tables: Arc<RwLock<HashMap<u32, PrivateTableState>>>,
    pub party_config_path: String,
    pub peer_http_endpoints: Vec<String>,
}

#[tokio::main]
async fn main() {
    let log_format = std::env::var("REQUEST_LOG_FORMAT").unwrap_or_default();
    if log_format.eq_ignore_ascii_case("json") {
        tracing_subscriber::fmt().json().init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let node_id: u32 = std::env::var("NODE_ID")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap();
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| format!("{}", 8101 + node_id))
        .parse()
        .unwrap();
    let party_config_path = std::env::var("PARTY_CONFIG")
        .unwrap_or_else(|_| format!("./config/party_{}.toml", node_id));
    let peer_http_endpoints = std::env::var("NODE_HTTP_ENDPOINTS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            vec![
                "http://localhost:8101".to_string(),
                "http://localhost:8102".to_string(),
                "http://localhost:8103".to_string(),
            ]
        });

    tracing::info!("MPC Node {} starting on port {}", node_id, port);
    tracing::info!("Party config: {}", party_config_path);
    tracing::info!("Peer HTTP endpoints: {:?}", peer_http_endpoints);

    let state = NodeState {
        node_id,
        sessions: Arc::new(RwLock::new(HashMap::new())),
        tables: Arc::new(RwLock::new(HashMap::new())),
        party_config_path,
        peer_http_endpoints,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/table/:table_id/prepare-deal",
            post(api::post_prepare_deal),
        )
        .route(
            "/table/:table_id/prepare-reveal/:phase",
            post(api::post_prepare_reveal),
        )
        .route(
            "/table/:table_id/prepare-showdown",
            post(api::post_prepare_showdown),
        )
        .route(
            "/table/:table_id/dispatch-shares",
            post(api::post_dispatch_shares),
        )
        .route("/table/:table_id/perm-lookup", post(api::post_perm_lookup))
        .route("/session/:id/shares", post(api::post_shares))
        .route("/session/:id/generate", post(api::post_generate))
        .route("/session/:id/status", get(api::get_status))
        .route("/session/:id/proof", get(api::get_proof))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

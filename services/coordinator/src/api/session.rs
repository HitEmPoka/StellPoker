use axum::http::StatusCode;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::hash::{Hash, Hasher};
use uuid;

use super::auth::is_valid_stellar_address;
use super::parsing::{map_onchain_phase_to_local, normalize_field_value, parse_u32_value};
use super::{MAX_PLAYERS, MIN_PLAYERS};
use crate::{chain_cache, soroban, AppState, TableSession};

pub(crate) async fn ensure_session_exists(
    state: &AppState,
    table_id: u32,
) -> Result<(), StatusCode> {
    {
        let tables = state.tables.read().await;
        if tables.contains_key(&table_id) {
            return Ok(());
        }
    }

    if !state.soroban_config.is_configured() {
        return Err(StatusCode::NOT_FOUND);
    }

    let raw_state = soroban::get_table_state(&state.soroban_config, table_id)
        .await
        .map_err(|e| {
            tracing::warn!(
                "failed to fetch on-chain table {} for session rehydrate: {}",
                table_id,
                e
            );
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    let restored = build_session_from_onchain_state(table_id, &raw_state).map_err(|e| {
        tracing::warn!(
            "failed to rehydrate table {} from on-chain state: {}",
            table_id,
            e
        );
        StatusCode::NOT_FOUND
    })?;

    let mut tables = state.tables.write().await;
    tables.entry(table_id).or_insert(restored);
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct OnchainTableView {
    pub phase: String,
    pub max_players: u32,
    pub seats: Vec<(u32, String)>,
    /// Chip stacks aligned with `seats` order (Issue #53).
    pub stacks: Vec<i64>,
}

fn parse_i64_value(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(n) = v.as_u64() {
        return Some(n as i64);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<i64>().ok();
    }
    None
}

pub(crate) async fn fetch_onchain_table_view(
    soroban_config: &soroban::SorobanConfig,
    table_id: u32,
) -> Result<OnchainTableView, String> {
    let raw_state = soroban::get_table_state(soroban_config, table_id).await?;
    parse_onchain_table_view(&raw_state)
}

/// Cached version of fetch_onchain_table_view using chain cache.
pub(crate) async fn fetch_onchain_table_view_cached(
    soroban_config: &soroban::SorobanConfig,
    chain_cache: &Arc<chain_cache::ChainCache>,
    table_id: u32,
) -> Result<OnchainTableView, String> {
    let soroban_config = soroban_config.clone();
    let raw_state = chain_cache
        .get_table_state(table_id, move |table_ids| {
            let soroban_config = soroban_config.clone();
            async move {
                let mut results = HashMap::new();
                for id in table_ids {
                    let result = soroban::get_table_state(&soroban_config, id).await;
                    results.insert(id, result);
                }
                results
            }
        })
        .await?;

    parse_onchain_table_view(&raw_state)
}

fn parse_onchain_table_view(raw_state: &str) -> Result<OnchainTableView, String> {
    let value: Value =
        serde_json::from_str(raw_state).map_err(|e| format!("invalid table json: {}", e))?;

    let phase = value
        .get("phase")
        .and_then(|v| v.as_str())
        .ok_or("missing phase")?
        .to_string();

    let mut seat_rows: Vec<(u32, String, i64)> = value
        .get("players")
        .and_then(|v| v.as_array())
        .ok_or("missing players")?
        .iter()
        .filter_map(|player| {
            let address = player.get("address")?.as_str()?.to_string();
            let seat = player
                .get("seat_index")
                .and_then(parse_u32_value)
                .unwrap_or(0);
            let stack = player.get("stack").and_then(parse_i64_value).unwrap_or(0);
            Some((seat, address, stack))
        })
        .collect();
    seat_rows.sort_by_key(|(seat, _, _)| *seat);

    let seats: Vec<(u32, String)> = seat_rows
        .iter()
        .map(|(seat, addr, _)| (*seat, addr.clone()))
        .collect();
    let stacks: Vec<i64> = seat_rows.iter().map(|(_, _, s)| *s).collect();

    let max_players = value
        .get("config")
        .and_then(|cfg| cfg.get("max_players"))
        .and_then(parse_u32_value)
        .unwrap_or_else(|| seats.len() as u32);

    Ok(OnchainTableView {
        phase,
        max_players,
        seats,
        stacks,
    })
}

pub(crate) async fn resolve_deal_players_from_lobby(
    state: &AppState,
    table_id: u32,
) -> Result<Vec<String>, StatusCode> {
    let view = fetch_onchain_table_view_cached(&state.soroban_config, &state.chain_cache, table_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let lobby = state.lobby_assignments.read().await;
    let table_lobby = lobby.get(&table_id);

    let mut ordered_players = Vec::new();
    for (_, chain_address) in &view.seats {
        let logical = table_lobby
            .and_then(|table| {
                table
                    .iter()
                    .find(|(_, mapped_chain)| *mapped_chain == chain_address)
                    .map(|(wallet, _)| wallet.clone())
            })
            .unwrap_or_else(|| chain_address.clone());
        ordered_players.push(logical);
    }

    if ordered_players.len() < MIN_PLAYERS {
        return Err(StatusCode::CONFLICT);
    }
    validate_players(&ordered_players)?;

    Ok(ordered_players)
}

fn build_session_from_onchain_state(
    table_id: u32,
    raw_state: &str,
) -> Result<TableSession, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw_state).map_err(|e| format!("invalid table json: {}", e))?;

    let phase_raw = value
        .get("phase")
        .and_then(|v| v.as_str())
        .ok_or("missing phase")?;
    let phase = map_onchain_phase_to_local(phase_raw)
        .ok_or_else(|| format!("unsupported on-chain phase '{}'", phase_raw))?;

    let mut seated: Vec<(u32, String)> = value
        .get("players")
        .and_then(|v| v.as_array())
        .ok_or("missing players")?
        .iter()
        .filter_map(|player| {
            let address = player.get("address")?.as_str()?.to_string();
            let seat = player
                .get("seat_index")
                .and_then(parse_u32_value)
                .unwrap_or(0);
            Some((seat, address))
        })
        .collect();
    seated.sort_by_key(|(seat, _)| *seat);
    let player_order: Vec<String> = seated.into_iter().map(|(_, address)| address).collect();

    if player_order.len() < MIN_PLAYERS {
        return Err(format!(
            "not enough seated players to restore session: {}",
            player_order.len()
        ));
    }

    let deck_root_raw = value
        .get("deck_root")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let deck_root = if deck_root_raw.is_empty() {
        String::new()
    } else {
        normalize_field_value(&deck_root_raw)?
    };

    if phase != "waiting" && phase != "dealing" && deck_root.is_empty() {
        return Err("missing deck_root for active hand".to_string());
    }

    let hand_commitments: Vec<String> = value
        .get("hand_commitments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str())
                .map(normalize_field_value)
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();

    let board_cards: Vec<u32> = value
        .get("board_cards")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_u32_value).collect())
        .unwrap_or_default();
    let board_count = board_cards.len();

    let mut hole_indices = Vec::with_capacity(player_order.len() * 2);
    let mut player_card_positions = Vec::with_capacity(player_order.len());
    for seat in 0..player_order.len() {
        let c1 = (seat * 2) as u32;
        let c2 = c1 + 1;
        player_card_positions.push((c1, c2));
        hole_indices.push(c1);
        hole_indices.push(c2);
    }

    let chain_dealt_indices: Vec<u32> = value
        .get("dealt_indices")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_u32_value).collect())
        .unwrap_or_default();

    let board_indices = if chain_dealt_indices.is_empty() {
        let start = (player_order.len() * 2) as u32;
        (0..board_count)
            .map(|i| start + i as u32)
            .collect::<Vec<u32>>()
    } else if chain_dealt_indices.len() >= hole_indices.len() + board_count {
        chain_dealt_indices[chain_dealt_indices.len() - board_count..].to_vec()
    } else {
        chain_dealt_indices.clone()
    };

    let dealt_indices = if chain_dealt_indices.is_empty() {
        let mut combined = hole_indices.clone();
        combined.extend(board_indices.iter().copied());
        combined
    } else if chain_dealt_indices.len() >= hole_indices.len() {
        chain_dealt_indices
    } else {
        let mut combined = hole_indices.clone();
        combined.extend(chain_dealt_indices.iter().copied());
        combined
    };

    let mut revealed_cards_by_phase = HashMap::new();
    if board_cards.len() >= 3 {
        revealed_cards_by_phase.insert("flop".to_string(), board_cards[0..3].to_vec());
    }
    if board_cards.len() >= 4 {
        revealed_cards_by_phase.insert("turn".to_string(), vec![board_cards[3]]);
    }
    if board_cards.len() >= 5 {
        revealed_cards_by_phase.insert("river".to_string(), vec![board_cards[4]]);
    }

    Ok(TableSession {
        table_id,
        deck_root,
        hand_commitments,
        player_order,
        dealt_indices,
        player_card_positions,
        board_indices,
        phase: phase.to_string(),
        deal_session_id: "rehydrated-from-chain".to_string(),
        deal_tx_hash: None,
        reveal_tx_hashes: HashMap::new(),
        reveal_session_ids: HashMap::new(),
        revealed_cards_by_phase,
        selected_node_endpoints: Vec::new(), // Will be populated on first MPC call if needed
        showdown_tx_hash: None,
        showdown_session_id: None,
        showdown_result: None,
        proof_nonce: 0,
        rit_phase: "inactive".to_string(),
        rit_shared_board_count: 0,
        mpc_node_progress: Vec::new(),
        mpc_operation_started: None,
        // Rehydrated sessions have no pinned hashes — they will be re-pinned
        // on the next deal. No proof generation happens until a new deal starts.
        pinned_artifact_hashes: HashMap::new(),
    })
}

/// Generate a cryptographically random, single-use proof session ID.
///
/// The ID embeds the table ID and a label for traceability, but the UUID
/// component ensures the full ID is unpredictable and non-replayable.
/// Session IDs must be checked against the `used_session_ids` set in
/// `AppState` before being forwarded to MPC nodes.
pub(crate) fn next_proof_session_id(session: &mut TableSession, label: &str) -> String {
    session.proof_nonce = session.proof_nonce.saturating_add(1);
    format!(
        "table-{}-{}-{}",
        session.table_id,
        label,
        uuid::Uuid::new_v4(),
    )
}

pub(crate) fn validate_table_id(_table_id: u32) -> Result<(), StatusCode> {
    Ok(())
}

pub(crate) fn validate_players(players: &[String]) -> Result<(), StatusCode> {
    if players.len() < MIN_PLAYERS || players.len() > MAX_PLAYERS {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut seen = HashSet::new();
    for address in players {
        if !is_valid_stellar_address(address) {
            return Err(StatusCode::BAD_REQUEST);
        }
        if !seen.insert(address) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    Ok(())
}

pub(crate) fn validate_reveal_phase(phase: &str) -> Result<(), StatusCode> {
    match phase {
        "flop" | "turn" | "river" => Ok(()),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

/// Whether dynamic MPC node discovery is active: it is used only when neither
/// the on-chain committee registry nor static `MPC_NODE_*` endpoints are
/// configured. This preserves backward compatibility — explicit static config
/// always takes precedence over runtime self-registration.
pub(crate) fn dynamic_discovery_active(state: &AppState) -> bool {
    state.soroban_config.committee_registry_contract.is_empty()
        && !state.mpc_config.static_endpoints_configured
}

/// Select 3 healthy MPC nodes, prioritizing those in the requested region.
pub(crate) async fn select_mpc_nodes(
    state: &AppState,
    table_id: u32,
    requested_region: Option<String>,
) -> Result<Vec<String>, StatusCode> {
    // Dynamic discovery: choose from nodes that self-registered at runtime and
    // are still heartbeating. The registry enforces the 3-healthy-node minimum
    // (returning None -> 503) just like the registry/static paths below.
    if dynamic_discovery_active(state) {
        let endpoints = state
            .node_registry
            .read()
            .await
            .select_session_nodes()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        return Ok(order_for_table(endpoints, table_id, state.mpc_config.table_affinity_enabled));
    }

    let all_nodes = if state.soroban_config.committee_registry_contract.is_empty() {
        // Fallback to static config if registry not configured
        state
            .mpc_config
            .node_endpoints
            .iter()
            .map(|ep| soroban::CommitteeMember {
                address: String::new(),
                stake: 0,
                endpoint: ep.clone(),
                region: "unknown".to_string(),
                active: true,
                slash_count: 0,
            })
            .collect()
    } else {
        soroban::fetch_active_nodes_from_registry(&state.soroban_config)
            .await
            .map_err(|e| {
                tracing::error!("Failed to fetch nodes from registry: {}", e);
                StatusCode::SERVICE_UNAVAILABLE
            })?
    };

    if !state.soroban_config.committee_registry_contract.is_empty() {
        let epoch = soroban::fetch_current_epoch_from_registry(&state.soroban_config)
            .await
            .map_err(|e| {
                tracing::error!("Failed to fetch committee epoch: {}", e);
                StatusCode::SERVICE_UNAVAILABLE
            })?
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        soroban::validate_epoch_members(&epoch, &all_nodes).map_err(|e| {
            tracing::error!("Committee membership mismatch: {}", e);
            StatusCode::SERVICE_UNAVAILABLE
        })?;
    }

    let healths = state.metrics.node_healths.lock().await;
    let mut healthy_members: Vec<soroban::CommitteeMember> = all_nodes
        .into_iter()
        .filter(|m| {
            healths
                .iter()
                .find(|h| h.endpoint == m.endpoint)
                .map(|h| h.connected)
                .unwrap_or(false)
        })
        .collect();

    if healthy_members.len() < 3 {
        tracing::warn!(
            "Not enough healthy MPC nodes: found {}",
            healthy_members.len()
        );
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Sort by region match
    if let Some(ref region) = requested_region {
        healthy_members.sort_by_key(|m| m.region != *region);
    }

    if state.mpc_config.table_affinity_enabled {
        healthy_members.sort_by_key(|member| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            table_id.hash(&mut hasher);
            member.endpoint.hash(&mut hasher);
            hasher.finish()
        });
    }

    let selected = healthy_members
        .into_iter()
        .take(3)
        .map(|m| m.endpoint)
        .collect();
    Ok(selected)
}

fn order_for_table(mut endpoints: Vec<String>, table_id: u32, enabled: bool) -> Vec<String> {
    if enabled {
        endpoints.sort_by_key(|endpoint| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            table_id.hash(&mut hasher);
            endpoint.hash(&mut hasher);
            hasher.finish()
        });
    }
    endpoints
}

pub(crate) fn is_identity_missing_error(error: &str) -> bool {
    error
        .to_ascii_lowercase()
        .contains("no local identity configured")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TableSession;
    use std::collections::HashSet;

    fn make_session(table_id: u32) -> TableSession {
        TableSession {
            table_id,
            deck_root: String::new(),
            hand_commitments: Vec::new(),
            player_order: Vec::new(),
            dealt_indices: Vec::new(),
            player_card_positions: Vec::new(),
            board_indices: Vec::new(),
            phase: "waiting".to_string(),
            deal_session_id: String::new(),
            deal_tx_hash: None,
            reveal_tx_hashes: std::collections::HashMap::new(),
            reveal_session_ids: std::collections::HashMap::new(),
            revealed_cards_by_phase: std::collections::HashMap::new(),
            selected_node_endpoints: Vec::new(),
            showdown_tx_hash: None,
            showdown_session_id: None,
            showdown_result: None,
            proof_nonce: 0,
            mpc_node_progress: Vec::new(),
            mpc_operation_started: None,
            pinned_artifact_hashes: std::collections::HashMap::new(),
            rit_phase: String::new(),
            rit_shared_board_count: 0,
        }
    }

    /// Session IDs must be unpredictable: two calls with the same label must
    /// yield different values (UUID component makes collisions astronomically
    /// unlikely — treat a collision as a test failure).
    #[test]
    fn session_ids_are_unique() {
        let mut session = make_session(42);
        let id1 = next_proof_session_id(&mut session, "deal");
        let id2 = next_proof_session_id(&mut session, "deal");
        assert_ne!(id1, id2, "session IDs must not repeat");
    }

    /// Session IDs must embed the table ID so they are scoped to a specific
    /// game and cannot be replayed against a different table.
    #[test]
    fn session_id_binds_to_table_id() {
        let mut s1 = make_session(1);
        let mut s2 = make_session(2);
        let id1 = next_proof_session_id(&mut s1, "deal");
        let id2 = next_proof_session_id(&mut s2, "deal");
        assert!(id1.contains("table-1-"), "ID should contain table-1-");
        assert!(id2.contains("table-2-"), "ID should contain table-2-");
        assert_ne!(id1, id2);
    }

    /// Simulate the single-use enforcement: inserting the same ID twice must
    /// be detected (HashSet::insert returns false on duplicate).
    #[test]
    fn used_session_ids_detects_replay() {
        let mut session = make_session(7);
        let id = next_proof_session_id(&mut session, "reveal-flop");

        let mut used: HashSet<String> = HashSet::new();
        assert!(used.insert(id.clone()), "first registration must succeed");
        assert!(!used.insert(id.clone()), "replay must be detected");
    }

    /// Verify 100 consecutive IDs are all distinct (no sequential pattern).
    #[test]
    fn session_ids_are_non_sequential() {
        let mut session = make_session(99);
        let ids: HashSet<String> = (0..100)
            .map(|_| next_proof_session_id(&mut session, "deal"))
            .collect();
        assert_eq!(ids.len(), 100, "all 100 IDs must be unique");
    }

    #[test]
    fn authenticated_card_delivery_blocks_malicious_coordinator_swap() {
        use ed25519_dalek::SigningKey;
        let committee_sk = SigningKey::from_bytes(&[7u8; 32]);
        let committee_vk = committee_sk.verifying_key();

        let player_a_secret = [11u8; 32];
        let player_a_pub = derive_session_pubkey(&player_a_secret);
        let player_b_secret = [22u8; 32];
        let player_b_pub = derive_session_pubkey(&player_b_secret);

        let addr_a = "GA_PLAYER_A_TEST";
        let addr_b = "GB_PLAYER_B_TEST";

        let delivery_a = build_authenticated_card_delivery(
            &player_a_pub,
            1,
            addr_a,
            14,
            27,
            "salt_a1",
            "salt_a2",
            &committee_sk,
        )
        .unwrap();

        let _delivery_b = build_authenticated_card_delivery(
            &player_b_pub,
            1,
            addr_b,
            2,
            3,
            "salt_b1",
            "salt_b2",
            &committee_sk,
        )
        .unwrap();

        // Honest decryption by Player A succeeds
        let cards_a = verify_and_decrypt_card_delivery(
            &delivery_a,
            1,
            addr_a,
            &player_a_secret,
            &committee_vk,
        )
        .unwrap();
        assert_eq!(cards_a, (14, 27, "salt_a1".to_string(), "salt_a2".to_string()));

        // Player B cannot decrypt Player A's delivery
        let err_wrong_key = verify_and_decrypt_card_delivery(
            &delivery_a,
            1,
            addr_a,
            &player_b_secret,
            &committee_vk,
        )
        .unwrap_err();
        assert!(err_wrong_key.contains("decrypt"), "{err_wrong_key}");

        // Malicious coordinator routes delivery_a to Player B without altering envelope
        let err_misaddressed = verify_and_decrypt_card_delivery(
            &delivery_a,
            1,
            addr_b,
            &player_b_secret,
            &committee_vk,
        )
        .unwrap_err();
        assert!(err_misaddressed.contains("mis-addressed"), "{err_misaddressed}");

        // Malicious coordinator swaps ciphertext into Player B's envelope
        let mut swapped = delivery_a.clone();
        swapped.recipient_address = addr_b.to_string();
        let err_swapped = verify_and_decrypt_card_delivery(
            &swapped,
            1,
            addr_b,
            &player_b_secret,
            &committee_vk,
        )
        .unwrap_err();
        assert!(err_swapped.contains("signature"), "{err_swapped}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Issue #882: Authenticated Private Card Delivery (Coordinator Side)
// ─────────────────────────────────────────────────────────────────────────────

/// Derive a 32-byte session public key from a 32-byte session secret key.
pub fn derive_session_pubkey(secret: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"STELLPOKER_SESSION_PK_V1:");
    h.update(secret);
    h.finalize().into()
}

/// Compute the canonical signing payload for committee threshold signature binding.
pub fn card_delivery_signing_bytes(
    table_id: u32,
    recipient_address: &str,
    ephemeral_pubkey_hex: &str,
    nonce_hex: &str,
    ciphertext_hex: &str,
) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"STELLPOKER_CARD_DELIVERY_V1:");
    h.update(&table_id.to_le_bytes());
    h.update(recipient_address.as_bytes());
    h.update(ephemeral_pubkey_hex.as_bytes());
    h.update(nonce_hex.as_bytes());
    h.update(ciphertext_hex.as_bytes());
    h.finalize().to_vec()
}

/// Encrypt hole cards to a player's session public key and sign with committee key.
#[allow(clippy::too_many_arguments)]
pub fn build_authenticated_card_delivery(
    recipient_session_pubkey: &[u8; 32],
    table_id: u32,
    recipient_address: &str,
    card1: u32,
    card2: u32,
    salt1: &str,
    salt2: &str,
    committee_sk: &ed25519_dalek::SigningKey,
) -> Result<super::types::AuthenticatedCardDelivery, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit, OsRng},
        Aes256Gcm, Key, Nonce,
    };
    use ed25519_dalek::Signer;
    use rand::RngCore;
    use sha2::{Digest, Sha256};

    let mut ephemeral_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut ephemeral_bytes);
    let ephemeral_hex = hex::encode(ephemeral_bytes);

    let mut key_hasher = Sha256::new();
    key_hasher.update(b"STELLPOKER_CARD_ENC_KEY_V1:");
    key_hasher.update(recipient_session_pubkey);
    key_hasher.update(&ephemeral_bytes);
    key_hasher.update(&table_id.to_le_bytes());
    key_hasher.update(recipient_address.as_bytes());
    let aes_key_bytes: [u8; 32] = key_hasher.finalize().into();

    let plaintext = serde_json::json!({
        "card1": card1,
        "card2": card2,
        "salt1": salt1,
        "salt2": salt2,
    })
    .to_string();

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce_hex = hex::encode(nonce_bytes);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key_bytes));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
        .map_err(|e| format!("encryption failed: {}", e))?;
    let ciphertext_hex = hex::encode(ciphertext);

    let signing_msg = card_delivery_signing_bytes(
        table_id,
        recipient_address,
        &ephemeral_hex,
        &nonce_hex,
        &ciphertext_hex,
    );
    let sig = committee_sk.sign(&signing_msg);

    Ok(super::types::AuthenticatedCardDelivery {
        table_id,
        recipient_address: recipient_address.to_string(),
        ephemeral_pubkey: ephemeral_hex,
        nonce: nonce_hex,
        ciphertext: ciphertext_hex,
        committee_signature: hex::encode(sig.to_bytes()),
        committee_pubkey: hex::encode(committee_sk.verifying_key().to_bytes()),
    })
}

/// Verify committee signature, recipient address, and decrypt hole cards.
pub fn verify_and_decrypt_card_delivery(
    delivery: &super::types::AuthenticatedCardDelivery,
    expected_table_id: u32,
    expected_recipient: &str,
    player_session_secret: &[u8; 32],
    committee_vk: &ed25519_dalek::VerifyingKey,
) -> Result<(u32, u32, String, String), String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Key, Nonce,
    };
    use ed25519_dalek::Verifier;
    use sha2::{Digest, Sha256};

    if delivery.table_id != expected_table_id {
        return Err(format!("table_id mismatch: {}", delivery.table_id));
    }
    if delivery.recipient_address != expected_recipient {
        return Err(format!(
            "mis-addressed delivery: expected {}, got {}",
            expected_recipient, delivery.recipient_address
        ));
    }

    let signing_msg = card_delivery_signing_bytes(
        delivery.table_id,
        &delivery.recipient_address,
        &delivery.ephemeral_pubkey,
        &delivery.nonce,
        &delivery.ciphertext,
    );
    let sig_bytes: [u8; 64] = hex::decode(&delivery.committee_signature)
        .map_err(|e| format!("invalid signature hex: {}", e))?
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    committee_vk
        .verify(&signing_msg, &ed25519_dalek::Signature::from_bytes(&sig_bytes))
        .map_err(|e| format!("committee signature invalid: {}", e))?;

    let recipient_pub = derive_session_pubkey(player_session_secret);
    let ephemeral_bytes = hex::decode(&delivery.ephemeral_pubkey)
        .map_err(|e| format!("invalid ephemeral pubkey: {}", e))?;
    let mut key_hasher = Sha256::new();
    key_hasher.update(b"STELLPOKER_CARD_ENC_KEY_V1:");
    key_hasher.update(recipient_pub);
    key_hasher.update(&ephemeral_bytes);
    key_hasher.update(&delivery.table_id.to_le_bytes());
    key_hasher.update(delivery.recipient_address.as_bytes());
    let aes_key_bytes: [u8; 32] = key_hasher.finalize().into();

    let nonce_bytes = hex::decode(&delivery.nonce).map_err(|e| format!("invalid nonce: {}", e))?;
    let ciphertext_bytes =
        hex::decode(&delivery.ciphertext).map_err(|e| format!("invalid ciphertext: {}", e))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key_bytes));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext_bytes.as_ref())
        .map_err(|_| "failed to decrypt payload: wrong key or corrupted ciphertext".to_string())?;

    let parsed: serde_json::Value = serde_json::from_slice(&plaintext)
        .map_err(|e| format!("invalid decrypted JSON: {}", e))?;
    Ok((
        parsed["card1"].as_u64().ok_or("missing card1")? as u32,
        parsed["card2"].as_u64().ok_or("missing card2")? as u32,
        parsed["salt1"].as_str().ok_or("missing salt1")?.to_string(),
        parsed["salt2"].as_str().ok_or("missing salt2")?.to_string(),
    ))
}

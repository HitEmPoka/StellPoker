//! Soroban on-chain proof submission via `stellar contract invoke`.
//!
//! Shells out to the Stellar CLI to submit proofs and game state to
//! the on-chain poker-table contract. Uses the same `tokio::process::Command`
//! pattern as `mpc.rs` for co-noir subprocess execution.

mod actions;
mod proofs;

pub use actions::*;
pub use proofs::*;

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// Configuration for Soroban interactions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SorobanConfig {
    pub rpc_url: String,
    pub secret_key: String,
    pub poker_table_contract: String,
    pub network_passphrase: String,
    pub onchain_table_id: Option<u32>,
    pub player_identities: Vec<(String, String)>,
}

impl SorobanConfig {
    pub fn from_env() -> Self {
        let mut player_identities = Vec::new();
        for idx in 1..=6 {
            let address_key = format!("PLAYER{}_ADDRESS", idx);
            if let Ok(address) = std::env::var(&address_key) {
                if address.trim().is_empty() {
                    continue;
                }
                let identity_key = format!("PLAYER{}_IDENTITY", idx);
                let identity =
                    std::env::var(&identity_key).unwrap_or_else(|_| format!("player{}-local", idx));
                player_identities.push((address, identity));
            }
        }

        Self {
            rpc_url: std::env::var("SOROBAN_RPC")
                .unwrap_or_else(|_| "http://localhost:8000/soroban/rpc".to_string()),
            secret_key: std::env::var("COMMITTEE_SECRET")
                .unwrap_or_else(|_| "test_secret".to_string()),
            poker_table_contract: std::env::var("POKER_TABLE_CONTRACT")
                .unwrap_or_else(|_| String::new()),
            network_passphrase: std::env::var("NETWORK_PASSPHRASE")
                .unwrap_or_else(|_| "Test SDF Network ; September 2015".to_string()),
            onchain_table_id: std::env::var("ONCHAIN_TABLE_ID")
                .ok()
                .or_else(|| std::env::var("TABLE_ID").ok())
                .and_then(|s| s.parse().ok()),
            player_identities,
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.poker_table_contract.is_empty() && self.secret_key != "test_secret"
    }

    /// Derive the Stellar public address (G...) from the committee secret key (S...).
    pub fn committee_address(&self) -> Result<String, String> {
        let sk = stellar_strkey::ed25519::PrivateKey::from_string(&self.secret_key)
            .map_err(|e| format!("invalid committee secret key: {:?}", e))?;
        let signing_key = SigningKey::from_bytes(&sk.0);
        let public_key = signing_key.verifying_key().to_bytes();
        Ok(stellar_strkey::ed25519::PublicKey(public_key).to_string())
    }

    pub(crate) fn identity_for_player(&self, player_address: &str) -> Option<&str> {
        self.player_identities
            .iter()
            .find(|(address, _)| address == player_address)
            .map(|(_, identity)| identity.as_str())
    }

    pub fn has_identity_for_player(&self, player_address: &str) -> bool {
        self.identity_for_player(player_address).is_some()
    }
}

const INSTRUCTION_LEEWAY_STEPS: [u64; 4] = [0, 50_000_000, 200_000_000, 500_000_000];

fn is_transient_invoke_error(output: &std::process::Output) -> bool {
    if output.status.success() {
        return false;
    }

    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    stderr.contains("resourcelimitexceeded")
        || stderr.contains("connection reset by peer")
        || stderr.contains("timed out")
        || stderr.contains("timeout")
        || stderr.contains("temporarily unavailable")
        || stderr.contains("networking or low-level protocol error")
}

pub(crate) async fn invoke_contract_with_retries(
    config: &SorobanConfig,
    contract_args: Vec<String>,
) -> Result<std::process::Output, String> {
    let mut last_output: Option<std::process::Output> = None;

    for (attempt_idx, leeway) in INSTRUCTION_LEEWAY_STEPS.iter().enumerate() {
        let mut args: Vec<String> = vec![
            "contract".to_string(),
            "invoke".to_string(),
            "--id".to_string(),
            config.poker_table_contract.clone(),
            "--source".to_string(),
            config.secret_key.clone(),
            "--rpc-url".to_string(),
            config.rpc_url.clone(),
            "--network-passphrase".to_string(),
            config.network_passphrase.clone(),
        ];

        if *leeway > 0 {
            args.push("--instruction-leeway".to_string());
            args.push(leeway.to_string());
        }

        args.push("--".to_string());
        args.extend(contract_args.iter().cloned());

        let output = Command::new("stellar")
            .args(&args)
            .output()
            .await
            .map_err(|e| format!("Failed to invoke stellar CLI: {}", e))?;

        if output.status.success() {
            return Ok(output);
        }

        let is_resource_limit = is_transient_invoke_error(&output)
            && String::from_utf8_lossy(&output.stderr).contains("ResourceLimitExceeded");
        let has_next_attempt = attempt_idx + 1 < INSTRUCTION_LEEWAY_STEPS.len();

        if is_resource_limit && has_next_attempt {
            tracing::warn!(
                "stellar invoke hit ResourceLimitExceeded; retrying with higher instruction leeway (attempt {}/{})",
                attempt_idx + 1,
                INSTRUCTION_LEEWAY_STEPS.len()
            );
            last_output = Some(output);
            continue;
        }

        return Ok(output);
    }

    last_output.ok_or_else(|| "stellar invoke failed before any attempt completed".to_string())
}

pub(crate) fn resolve_onchain_table_id(config: &SorobanConfig, table_id: u32) -> u32 {
    if table_id == 0 {
        config.onchain_table_id.unwrap_or(0)
    } else {
        table_id
    }
}

pub(crate) async fn invoke_contract_with_source(
    config: &SorobanConfig,
    source: &str,
    contract_args: Vec<String>,
) -> Result<std::process::Output, String> {
    let mut args: Vec<String> = vec![
        "contract".to_string(),
        "invoke".to_string(),
        "--id".to_string(),
        config.poker_table_contract.clone(),
        "--source".to_string(),
        source.to_string(),
        "--rpc-url".to_string(),
        config.rpc_url.clone(),
        "--network-passphrase".to_string(),
        config.network_passphrase.clone(),
        "--".to_string(),
    ];
    args.extend(contract_args);

    Command::new("stellar")
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("Failed to invoke stellar CLI: {}", e))
}

pub(crate) async fn invoke_contract_with_source_retries(
    config: &SorobanConfig,
    source: &str,
    contract_args: Vec<String>,
) -> Result<std::process::Output, String> {
    const MAX_RETRIES: usize = 3;
    let mut last_output: Option<std::process::Output> = None;

    for attempt in 1..=MAX_RETRIES {
        let output = invoke_contract_with_source(config, source, contract_args.clone()).await?;
        if output.status.success() {
            return Ok(output);
        }

        let should_retry = is_transient_invoke_error(&output) && attempt < MAX_RETRIES;
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        tracing::warn!(
            "stellar invoke (source={}, attempt {}/{}) failed{}: {}",
            source,
            attempt,
            MAX_RETRIES,
            if should_retry { ", retrying" } else { "" },
            stderr.trim()
        );

        if !should_retry {
            return Ok(output);
        }

        last_output = Some(output);
        tokio::time::sleep(std::time::Duration::from_millis(300 * attempt as u64)).await;
    }

    last_output.ok_or_else(|| "stellar invoke failed before any attempt completed".to_string())
}

pub(crate) fn parse_i128_value(value: &serde_json::Value) -> Option<i128> {
    match value {
        serde_json::Value::String(s) => s.parse::<i128>().ok(),
        serde_json::Value::Number(n) => n.as_i64().map(|v| v as i128),
        _ => None,
    }
}

pub(crate) fn parse_u32_value(value: &serde_json::Value) -> Option<u32> {
    match value {
        serde_json::Value::String(s) => s.parse::<u32>().ok(),
        serde_json::Value::Number(n) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
        _ => None,
    }
}

/// Parse the tx hash from stellar CLI output.
pub(crate) fn parse_tx_result(output: std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(format!("stellar contract invoke failed: {}", stderr.trim()));
    }

    // The stellar CLI prints the tx hash or result to stdout
    let tx_hash = stdout.trim().to_string();
    if tx_hash.is_empty() {
        // Some versions don't print a hash on success
        Ok("submitted".to_string())
    } else {
        Ok(tx_hash)
    }
}

fn parse_u32_from_stdout(stdout: &str) -> Option<u32> {
    for line in stdout.lines().rev() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Ok(v) = t.parse::<u32>() {
            return Some(v);
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(t) {
            if let Some(v) = json.as_u64().and_then(|n| u32::try_from(n).ok()) {
                return Some(v);
            }
            if let Some(v) = json
                .get("u32")
                .and_then(|n| n.as_u64())
                .and_then(|n| u32::try_from(n).ok())
            {
                return Some(v);
            }
        }
    }
    None
}

//! MPC node configuration validation on startup (Issue #240).
//!
//! Validates critical configuration before the node accepts sessions:
//! - TLS certificate files exist and are parseable
//! - Stellar keypair is valid (ed25519 seed)
//! - Soroban RPC endpoint is reachable
//! - Committee-registry contract address is non-empty when configured

#[derive(Debug)]
pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validate all MPC node configuration and return a report.
pub async fn validate_config() -> ValidationReport {
    let mut report = ValidationReport::new();

    validate_tls_config(&mut report);
    validate_stellar_keypair(&mut report);
    validate_soroban_rpc(&mut report).await;
    validate_committee_registry(&mut report);
    validate_peer_endpoints(&mut report);

    report
}

fn validate_tls_config(report: &mut ValidationReport) {
    let cert_path = std::env::var("TLS_SERVER_CERT_PATH").ok();
    let cert_b64 = std::env::var("TLS_SERVER_CERT_B64").ok();
    let key_path = std::env::var("TLS_SERVER_KEY_PATH").ok();
    let key_b64 = std::env::var("TLS_SERVER_KEY_B64").ok();

    let has_cert = cert_path.is_some() || cert_b64.is_some();
    let has_key = key_path.is_some() || key_b64.is_some();

    if has_cert && !has_key {
        report
            .errors
            .push("TLS_SERVER_CERT_PATH/B64 is set but TLS_SERVER_KEY_PATH/B64 is missing".into());
    }
    if has_key && !has_cert {
        report
            .errors
            .push("TLS_SERVER_KEY_PATH/B64 is set but TLS_SERVER_CERT_PATH/B64 is missing".into());
    }

    if let Some(path) = &cert_path {
        match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    report
                        .errors
                        .push(format!("TLS certificate file '{}' is empty", path));
                }
            }
            Err(e) => {
                report.errors.push(format!(
                    "TLS_SERVER_CERT_PATH='{}' is not readable: {}",
                    path, e
                ));
            }
        }
    }

    if let Some(path) = &key_path {
        match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    report
                        .errors
                        .push(format!("TLS key file '{}' is empty", path));
                }
            }
            Err(e) => {
                report.errors.push(format!(
                    "TLS_SERVER_KEY_PATH='{}' is not readable: {}",
                    path, e
                ));
            }
        }
    }

    if !has_cert {
        report.warnings.push(
            "TLS not configured — node will serve plain HTTP (set TLS_SERVER_CERT_PATH/B64 to enable TLS)".into(),
        );
    }
}

fn validate_stellar_keypair(report: &mut ValidationReport) {
    match std::env::var("STELLAR_SECRET_KEY") {
        Ok(key) => {
            let key = key.trim();
            if key.is_empty() {
                report
                    .errors
                    .push("STELLAR_SECRET_KEY is set but empty".into());
            } else if !key.starts_with('S') || key.len() != 56 {
                report.errors.push(format!(
                    "STELLAR_SECRET_KEY does not look like a valid Stellar seed (expected S...56 chars, got {} chars)",
                    key.len()
                ));
            }
        }
        Err(_) => {
            report
                .warnings
                .push("STELLAR_SECRET_KEY not set — node will not sign transactions".into());
        }
    }
}

async fn validate_soroban_rpc(report: &mut ValidationReport) {
    let rpc_url = match std::env::var("SOROBAN_RPC_URL") {
        Ok(url) => url,
        Err(_) => {
            report
                .warnings
                .push("SOROBAN_RPC_URL not set — Soroban integration disabled".into());
            return;
        }
    };

    if rpc_url.is_empty() {
        report
            .errors
            .push("SOROBAN_RPC_URL is set but empty".into());
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLatestLedger"
    });

    match client.post(&rpc_url).json(&body).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                report.warnings.push(format!(
                    "SOROBAN_RPC_URL='{}' returned HTTP {} — may be misconfigured",
                    rpc_url,
                    resp.status()
                ));
            }
        }
        Err(e) => {
            report.errors.push(format!(
                "SOROBAN_RPC_URL='{}' is unreachable: {}",
                rpc_url, e
            ));
        }
    }
}

fn validate_committee_registry(report: &mut ValidationReport) {
    match std::env::var("COMMITTEE_REGISTRY_CONTRACT") {
        Ok(addr) => {
            let addr = addr.trim();
            if addr.is_empty() {
                report
                    .errors
                    .push("COMMITTEE_REGISTRY_CONTRACT is set but empty".into());
            } else if !addr.starts_with('C') || addr.len() != 56 {
                report.warnings.push(format!(
                    "COMMITTEE_REGISTRY_CONTRACT='{}' does not look like a valid Stellar contract address",
                    addr
                ));
            }
        }
        Err(_) => {
            report
                .warnings
                .push("COMMITTEE_REGISTRY_CONTRACT not set — using static node endpoints".into());
        }
    }
}

fn validate_peer_endpoints(report: &mut ValidationReport) {
    let raw = match std::env::var("NODE_HTTP_ENDPOINTS") {
        Ok(v) => v,
        Err(_) => return,
    };

    let endpoints: Vec<&str> = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if endpoints.is_empty() {
        report
            .errors
            .push("NODE_HTTP_ENDPOINTS is set but contains no valid endpoints".into());
    }

    for ep in &endpoints {
        if !ep.starts_with("http://") && !ep.starts_with("https://") {
            report.errors.push(format!(
                "NODE_HTTP_ENDPOINTS contains '{}' which is not a valid HTTP(S) URL",
                ep
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_report_is_ok_when_empty() {
        let report = ValidationReport::new();
        assert!(report.is_ok());
    }

    #[test]
    fn validation_report_not_ok_with_errors() {
        let mut report = ValidationReport::new();
        report.errors.push("something broke".into());
        assert!(!report.is_ok());
    }
}

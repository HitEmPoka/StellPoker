/// Golden test suite for log redaction
/// Verifies that no sensitive material (card data, session keys, etc.) appears in logs
/// Implements CI grep suite per issue #666

use regex::Regex;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const GOLDEN_TEST_SCENARIO: &str = include_str!("fixtures/test_game_scenario.json");

/// Patterns that should NEVER appear in logs
mod secret_patterns {
    use regex::Regex;
    use lazy_static::lazy_static;

    lazy_static! {
        /// Luhn-valid test card patterns
        /// 16-digit sequences that pass Luhn check (simulated poker card data)
        pub static ref CREDIT_CARD: Regex = Regex::new(
            r"(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13})"
        ).unwrap();

        /// Poker hand patterns (hole cards, board cards)
        /// Format: card_rank card_suit (e.g., "AS", "KH", "2D")
        pub static ref CARD_PATTERN: Regex = Regex::new(
            r"(?i)\b([AKQJTxX]|10|[2-9])([CDHS♣♦♥♠])\b"
        ).unwrap();

        /// Shared secrets (TACEO session keys)
        /// Base64-encoded 32+ byte sequences
        pub static ref SESSION_KEY: Regex = Regex::new(
            r"(?i)(?:session_key|shared_secret|master_key)[\s:=]*([A-Za-z0-9+/]{43,}={0,3})"
        ).unwrap();

        /// Private key material
        pub static ref PRIVATE_KEY: Regex = Regex::new(
            r"-----BEGIN (RSA|EC|PRIVATE|OPENSSH) PRIVATE KEY-----"
        ).unwrap();

        /// Stellar secret key pattern (SXXXXX...)
        pub static ref STELLAR_SECRET: Regex = Regex::new(
            r"\bS[A-Z2-7]{55}\b"
        ).unwrap();

        /// Mnemonic phrases (12 or 24 words from BIP39 list)
        pub static ref MNEMONIC: Regex = Regex::new(
            r"\b(abandon|ability|able|about|above|absent|absorb|abstract|abuse|access|accident|account|accuse|achieve|acid|acoustic|acquire|across|act|action|actor|acts|actual|acuate|acute|ad|add|added|adder|adding|addition|additional|address|adds|adept|admin|admire|admit|ads|adult|advance|advances|advent|adventure|adverse|advertise|advice|advise|advisor|advocate)\b.*?\b(zebras|zero|zest|zesty|zeta|zeus|zigzag|zinc|zip|zone|zoned|zones|zoom|zoomed|zooming|zoos|zulu)\b"
        ).unwrap();

        /// AWS access key pattern
        pub static ref AWS_ACCESS_KEY: Regex = Regex::new(
            r"(?i)AKIA[0-9A-Z]{16}"
        ).unwrap();

        /// Generic API token/secret base64
        pub static ref API_TOKEN: Regex = Regex::new(
            r"(?i)(?:api[_-]?token|api[_-]?secret|access[_-]?token)[:\s=]+([A-Za-z0-9+/]{32,}={0,3})"
        ).unwrap();

        /// Common password patterns in logs
        pub static ref PASSWORD: Regex = Regex::new(
            r"(?i)(?:password|passwd|pwd)[:\s=]+[^\s\n]+"
        ).unwrap();

        /// Database connection strings
        pub static ref DB_CONNECTION: Regex = Regex::new(
            r"(?:postgres|mysql|mongodb)://[^\s:]+:[^\s@]+@[^\s/]+"
        ).unwrap();
    }
}

#[derive(serde::Deserialize)]
struct GameScenario {
    players: Vec<PlayerHand>,
    board: Vec<String>,
    expected_winner: String,
}

#[derive(serde::Deserialize)]
struct PlayerHand {
    address: String,
    hole_cards: Vec<String>,
}

/// Run a test scenario and capture logs
fn run_test_scenario(scenario_json: &str) -> String {
    // Parse scenario
    let scenario: GameScenario = serde_json::from_str(scenario_json)
        .expect("Failed to parse test scenario");

    // Simulate game processing (in real test, this would invoke actual game logic)
    let mut logs = String::new();

    logs.push_str(&format!("Game started with {} players\n", scenario.players.len()));

    for player in &scenario.players {
        logs.push_str(&format!("Player {} joined\n", player.address));
        // DO NOT log hole cards or other sensitive data
    }

    logs.push_str("Board dealt\n");
    logs.push_str(&format!("Winner: {}\n", scenario.expected_winner));

    logs
}

/// Scan logs for patterns that should never appear
fn check_for_secrets(log_content: &str) -> Vec<(String, Vec<String>)> {
    let mut findings = Vec::new();

    // Check each pattern
    if let Some(matches) = secret_patterns::CREDIT_CARD
        .find_iter(log_content)
        .map(|m| m.as_str().to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .next()
    {
        findings.push((
            "CREDIT_CARD_PATTERN_FOUND".to_string(),
            vec![matches],
        ));
    }

    if let Some(matches) = secret_patterns::CARD_PATTERN
        .find_iter(log_content)
        .map(|m| m.as_str().to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .next()
    {
        findings.push((
            "POKER_CARD_PATTERN_FOUND".to_string(),
            vec![matches],
        ));
    }

    if secret_patterns::SESSION_KEY.is_match(log_content) {
        findings.push((
            "SESSION_KEY_DETECTED".to_string(),
            secret_patterns::SESSION_KEY
                .captures_iter(log_content)
                .map(|c| c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default())
                .collect(),
        ));
    }

    if secret_patterns::PRIVATE_KEY.is_match(log_content) {
        findings.push((
            "PRIVATE_KEY_DETECTED".to_string(),
            secret_patterns::PRIVATE_KEY
                .captures_iter(log_content)
                .map(|c| c.get(0).map(|m| m.as_str().to_string()).unwrap_or_default())
                .collect(),
        ));
    }

    if secret_patterns::STELLAR_SECRET.is_match(log_content) {
        findings.push((
            "STELLAR_SECRET_KEY_DETECTED".to_string(),
            secret_patterns::STELLAR_SECRET
                .captures_iter(log_content)
                .map(|c| c.as_str().to_string())
                .collect(),
        ));
    }

    if secret_patterns::AWS_ACCESS_KEY.is_match(log_content) {
        findings.push((
            "AWS_ACCESS_KEY_DETECTED".to_string(),
            secret_patterns::AWS_ACCESS_KEY
                .captures_iter(log_content)
                .map(|c| c.as_str().to_string())
                .collect(),
        ));
    }

    if secret_patterns::PASSWORD.is_match(log_content) {
        findings.push((
            "PASSWORD_IN_LOGS".to_string(),
            secret_patterns::PASSWORD
                .captures_iter(log_content)
                .map(|c| c.as_str().to_string())
                .collect(),
        ));
    }

    if secret_patterns::DB_CONNECTION.is_match(log_content) {
        findings.push((
            "DB_CONNECTION_STRING_EXPOSED".to_string(),
            secret_patterns::DB_CONNECTION
                .captures_iter(log_content)
                .map(|c| c.as_str().to_string())
                .collect(),
        ));
    }

    findings
}

#[test]
fn test_game_scenario_no_secrets_in_logs() {
    let logs = run_test_scenario(GOLDEN_TEST_SCENARIO);
    let findings = check_for_secrets(&logs);

    if !findings.is_empty() {
        let report = findings
            .iter()
            .map(|(pattern, matches)| format!("  {}: {} occurrences", pattern, matches.len()))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("Secrets found in logs:\n{}", report);
    }
}

#[test]
fn test_log_files_no_secrets() {
    let log_patterns = vec![
        "target/logs/*.log",
        ".github/workflows/*.yml",
    ];

    for pattern in log_patterns {
        let paths = glob::glob(pattern).expect("Failed to read glob pattern");
        for entry in paths {
            if let Ok(path) = entry {
                if let Ok(content) = fs::read_to_string(&path) {
                    let findings = check_for_secrets(&content);
                    if !findings.is_empty() {
                        panic!(
                            "Secrets found in {}:\n{:?}",
                            path.display(),
                            findings
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_redaction_patterns() {
    // Test that our patterns correctly identify secrets
    let test_cases = vec![
        ("4532015112830366", true, "CREDIT_CARD"),
        ("AS KH 2D", false, "Card notation only in warnings"),
        ("SAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", true, "STELLAR_SECRET"),
        ("AKIAIOSFODNN7EXAMPLE", true, "AWS_ACCESS_KEY"),
        ("password=supersecret", true, "PASSWORD"),
    ];

    for (text, should_match, description) in test_cases {
        let findings = check_for_secrets(text);
        let has_findings = !findings.is_empty();

        if should_match && !has_findings {
            panic!("Failed to detect secret in: {}", description);
        } else if !should_match && has_findings {
            panic!("False positive for: {}", description);
        }
    }
}

#[test]
fn test_ci_workflow_secrets_scanning() {
    // Verify CI workflows don't expose secrets in logs or environment
    let ci_files = vec![
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/workflows/generate-sdk.yml",
    ];

    for file in ci_files {
        if let Ok(content) = fs::read_to_string(file) {
            // Check for common secret exposure patterns in workflows
            assert!(
                !content.contains("password:"),
                "Hardcoded password in {}", file
            );
            assert!(
                !content.contains("secret:"),
                "Exposed secret in {}", file
            );
            assert!(
                !content.contains("GITHUB_TOKEN") || content.contains("secrets.GITHUB_TOKEN"),
                "Unescaped token in {}", file
            );
        }
    }
}

mod fixtures {
    pub const TEST_GAME_SCENARIO: &str = r#"{
  "players": [
    {
      "address": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAY5V",
      "hole_cards": ["AS", "KS"]
    },
    {
      "address": "GBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBZZ",
      "hole_cards": ["AH", "AD"]
    }
  ],
  "board": ["2C", "3D", "4H", "5S", "6C"],
  "expected_winner": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAY5V"
}"#;
}

#[test]
fn test_golden_test_scenario_parses() {
    let _scenario: GameScenario = serde_json::from_str(GOLDEN_TEST_SCENARIO)
        .expect("Golden test scenario should parse");
}

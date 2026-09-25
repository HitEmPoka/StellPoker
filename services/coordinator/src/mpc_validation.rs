//! Validation of MPC output after reconstruction (Issue #139).
//!
//! `mpc::resolve_hole_cards` rebuilds a player's hole cards by chaining the
//! three nodes' permutation lookups and summing their salt shares. A faulty
//! or byzantine node can return a wrong permutation entry or salt share, and
//! the reconstruction would silently hand the player the wrong cards.
//!
//! The deal proof already binds the correct result: for each player it
//! publishes `hand_commitment = commit_hand(commit_card(c1, s1),
//! commit_card(c2, s2))` (see `circuits/lib/src/commitments.nr`), where
//! `commit_card(card, salt) = poseidon2_permutation([card, salt, 0, 0])[0]`
//! over BN254. This module recomputes that commitment from the reconstructed
//! cards and salts and rejects any mismatch. [`resolve_validated`] retries the
//! reconstruction, logs every failure, and escalates to an alert once a table
//! keeps failing.
//!
//! `MPC_RECONSTRUCTION_VALIDATION` selects the mode: `enforce` (default),
//! `warn` (log only, still return the output), or `off`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{LazyLock, Mutex};

use ark_bn254_poseidon::Fr;
use ark_ff_poseidon::{BigInteger, PrimeField};

/// Reconstruction attempts per request before giving up.
pub const MAX_RECONSTRUCTION_ATTEMPTS: u32 = 3;
/// Consecutive failed requests for one table that raise an alert.
pub const ALERT_AFTER_CONSECUTIVE_FAILURES: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationMode {
    Enforce,
    Warn,
    Off,
}

impl ValidationMode {
    pub fn from_env() -> Self {
        match std::env::var("MPC_RECONSTRUCTION_VALIDATION")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "warn" => Self::Warn,
            "off" | "0" | "false" => Self::Off,
            _ => Self::Enforce,
        }
    }
}

/// Parses a field element given as `0x`-prefixed hex (proof public inputs)
/// or decimal (reconstructed salts). Values must be canonical (< modulus).
pub fn parse_field(value: &str) -> Result<Fr, String> {
    let value = value.trim();
    let bytes = if let Some(hex_digits) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        let padded = if hex_digits.len() % 2 == 1 {
            format!("0{hex_digits}")
        } else {
            hex_digits.to_string()
        };
        hex::decode(&padded).map_err(|e| format!("invalid hex field element {value:?}: {e}"))?
    } else {
        decimal_to_be_bytes(value)?
    };
    if bytes.len() > 32 {
        return Err(format!("field element {value:?} exceeds 32 bytes"));
    }
    let fr = Fr::from_be_bytes_mod_order(&bytes);
    let canonical = fr.into_bigint().to_bytes_be();
    let mut padded = vec![0u8; 32 - bytes.len()];
    padded.extend_from_slice(&bytes);
    if canonical != padded {
        return Err(format!(
            "field element {value:?} is not below the BN254 modulus"
        ));
    }
    Ok(fr)
}

fn decimal_to_be_bytes(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("invalid decimal field element {value:?}"));
    }
    // Big-endian base-256 accumulator; field elements are at most 78 digits.
    let mut bytes: Vec<u8> = vec![0];
    for digit in value.bytes().map(|b| b - b'0') {
        let mut carry = digit as u32;
        for byte in bytes.iter_mut().rev() {
            let v = (*byte as u32) * 10 + carry;
            *byte = (v & 0xff) as u8;
            carry = v >> 8;
        }
        while carry > 0 {
            bytes.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
        if bytes.len() > 33 {
            return Err(format!("decimal field element {value:?} is too large"));
        }
    }
    Ok(bytes)
}

/// `poseidon2_permutation([a, b, 0, 0], 4)[0]`, as in `commitments.nr`.
fn poseidon2_hash_2(a: Fr, b: Fr) -> Fr {
    taceo_poseidon2::bn254::t4::permutation(&[a, b, Fr::from(0u64), Fr::from(0u64)])[0]
}

pub fn commit_card(card: u32, salt: Fr) -> Fr {
    poseidon2_hash_2(Fr::from(card as u64), salt)
}

pub fn commit_hand(card1_commit: Fr, card2_commit: Fr) -> Fr {
    poseidon2_hash_2(card1_commit, card2_commit)
}

/// Checks reconstructed hole cards and salts against the player's published
/// hand commitment.
pub fn verify_hole_cards(
    cards: &[u32],
    salts: &[String],
    expected_commitment: &str,
) -> Result<(), String> {
    if cards.len() != 2 || salts.len() != 2 {
        return Err(format!(
            "expected 2 cards and 2 salts, got {} and {}",
            cards.len(),
            salts.len()
        ));
    }
    let expected = parse_field(expected_commitment)?;
    let actual = commit_hand(
        commit_card(cards[0], parse_field(&salts[0])?),
        commit_card(cards[1], parse_field(&salts[1])?),
    );
    if actual != expected {
        return Err(
            "reconstructed hole cards do not match the published hand commitment".to_string(),
        );
    }
    Ok(())
}

/// Tracks consecutive failed requests per table so repeated failures raise
/// an alert instead of a stream of individual warnings.
pub struct FailureTracker {
    consecutive: Mutex<HashMap<u32, u32>>,
}

impl FailureTracker {
    pub fn new() -> Self {
        Self {
            consecutive: Mutex::new(HashMap::new()),
        }
    }

    /// Records a failure; returns the table's consecutive failure count.
    pub fn record_failure(&self, table_id: u32) -> u32 {
        let mut map = self.consecutive.lock().unwrap_or_else(|e| e.into_inner());
        let count = map.entry(table_id).or_insert(0);
        *count += 1;
        *count
    }

    pub fn record_success(&self, table_id: u32) {
        let mut map = self.consecutive.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&table_id);
    }
}

impl Default for FailureTracker {
    fn default() -> Self {
        Self::new()
    }
}

static FAILURES: LazyLock<FailureTracker> = LazyLock::new(FailureTracker::new);

/// Runs `resolve` up to [`MAX_RECONSTRUCTION_ATTEMPTS`] times until its
/// output matches `expected_commitment`. Every failed attempt is logged;
/// after [`ALERT_AFTER_CONSECUTIVE_FAILURES`] failed requests in a row for
/// the same table an `alert = "mpc_reconstruction_failure"` error is logged
/// for operators.
pub async fn resolve_validated<F, Fut>(
    table_id: u32,
    expected_commitment: Option<&str>,
    mode: ValidationMode,
    resolve: F,
) -> Result<(Vec<u32>, Vec<String>), String>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<(Vec<u32>, Vec<String>), String>>,
{
    resolve_validated_with(&FAILURES, table_id, expected_commitment, mode, resolve).await
}

pub async fn resolve_validated_with<F, Fut>(
    tracker: &FailureTracker,
    table_id: u32,
    expected_commitment: Option<&str>,
    mode: ValidationMode,
    resolve: F,
) -> Result<(Vec<u32>, Vec<String>), String>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<(Vec<u32>, Vec<String>), String>>,
{
    if mode == ValidationMode::Off {
        return resolve().await;
    }
    let Some(expected) = expected_commitment else {
        tracing::warn!(
            table_id,
            "no hand commitment recorded; MPC output not validated"
        );
        return resolve().await;
    };

    let mut last_error = String::new();
    for attempt in 1..=MAX_RECONSTRUCTION_ATTEMPTS {
        let (cards, salts) = resolve().await?;
        match verify_hole_cards(&cards, &salts, expected) {
            Ok(()) => {
                tracker.record_success(table_id);
                return Ok((cards, salts));
            }
            Err(e) if mode == ValidationMode::Warn => {
                tracing::warn!(table_id, error = %e, "MPC output failed validation (warn mode; returned anyway)");
                return Ok((cards, salts));
            }
            Err(e) => {
                tracing::warn!(
                    table_id,
                    attempt,
                    max_attempts = MAX_RECONSTRUCTION_ATTEMPTS,
                    error = %e,
                    "MPC reconstruction failed validation"
                );
                last_error = e;
            }
        }
    }

    let failures = tracker.record_failure(table_id);
    if failures >= ALERT_AFTER_CONSECUTIVE_FAILURES {
        tracing::error!(
            alert = "mpc_reconstruction_failure",
            table_id,
            consecutive_failures = failures,
            "MPC reconstruction keeps failing validation; a committee node may be faulty or byzantine"
        );
    }
    Err(format!(
        "MPC reconstruction rejected after {MAX_RECONSTRUCTION_ATTEMPTS} attempts: {last_error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn hex(fr: Fr) -> String {
        format!("0x{}", ::hex::encode(fr.into_bigint().to_bytes_be()))
    }

    fn hand(cards: [u32; 2], salts: [&str; 2]) -> String {
        hex(commit_hand(
            commit_card(cards[0], parse_field(salts[0]).unwrap()),
            commit_card(cards[1], parse_field(salts[1]).unwrap()),
        ))
    }

    #[test]
    fn poseidon2_matches_barretenberg_test_vector() {
        // poseidon2_permutation([0, 1, 2, 3]) from barretenberg's Poseidon2
        // tests — the permutation Noir's std::hash::poseidon2_permutation uses.
        let out = taceo_poseidon2::bn254::t4::permutation(&[
            Fr::from(0u64),
            Fr::from(1u64),
            Fr::from(2u64),
            Fr::from(3u64),
        ]);
        assert_eq!(
            hex(out[0]),
            "0x01bd538c2ee014ed5141b29e9ae240bf8db3fe5b9a38629a9647cf8d76c01737"
        );
    }

    /// The commitment helpers above must agree with the circuits. The shared
    /// vectors in circuits/test-vectors are generated once and asserted by
    /// the Noir library too (Issue #527), so this is the Rust half of that
    /// cross-check.
    #[test]
    fn commitments_match_shared_test_vectors() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../circuits/test-vectors/vectors.json");
        let text = std::fs::read_to_string(&path).expect("shared vectors present");
        let vectors: serde_json::Value = serde_json::from_str(&text).expect("valid vectors json");
        assert_eq!(vectors["schema"], "stellpoker.test-vectors/v1");

        let cards = vectors["commit_card"]
            .as_array()
            .expect("commit_card vectors");
        assert!(!cards.is_empty());
        for v in cards {
            let card = v["card"].as_u64().unwrap() as u32;
            let salt = parse_field(v["salt"].as_str().unwrap()).unwrap();
            assert_eq!(hex(commit_card(card, salt)), v["out"].as_str().unwrap());
        }

        let hands = vectors["commit_hand"]
            .as_array()
            .expect("commit_hand vectors");
        assert!(!hands.is_empty());
        for v in hands {
            let c1 = commit_card(
                v["card1"].as_u64().unwrap() as u32,
                parse_field(v["salt1"].as_str().unwrap()).unwrap(),
            );
            let c2 = commit_card(
                v["card2"].as_u64().unwrap() as u32,
                parse_field(v["salt2"].as_str().unwrap()).unwrap(),
            );
            assert_eq!(hex(commit_hand(c1, c2)), v["out"].as_str().unwrap());
        }
    }

    #[test]
    fn parses_hex_and_decimal_field_elements() {
        assert_eq!(parse_field("0x2a").unwrap(), Fr::from(42u64));
        assert_eq!(parse_field("42").unwrap(), Fr::from(42u64));
        assert_eq!(
            parse_field("55340232221128654845").unwrap(),
            Fr::from(55_340_232_221_128_654_845u128)
        );
        assert!(parse_field("").is_err());
        assert!(parse_field("12a").is_err());
        assert!(parse_field("0xzz").is_err());
        // The BN254 scalar modulus itself is not canonical.
        assert!(parse_field(
            "21888242871839275222246405745257275088548364400416034343698204186575808495617"
        )
        .is_err());
    }

    #[test]
    fn accepts_matching_reconstruction() {
        let commitment = hand([7, 51], ["123", "55340232221128654845"]);
        assert!(verify_hole_cards(
            &[7, 51],
            &["123".into(), "55340232221128654845".into()],
            &commitment
        )
        .is_ok());
    }

    #[test]
    fn rejects_wrong_card_or_salt() {
        let commitment = hand([7, 51], ["123", "456"]);
        assert!(verify_hole_cards(&[8, 51], &["123".into(), "456".into()], &commitment).is_err());
        assert!(verify_hole_cards(&[7, 51], &["124".into(), "456".into()], &commitment).is_err());
        // Swapped order is a different commitment.
        assert!(verify_hole_cards(&[51, 7], &["456".into(), "123".into()], &commitment).is_err());
        assert!(verify_hole_cards(&[7], &["123".into()], &commitment).is_err());
    }

    fn good() -> (Vec<u32>, Vec<String>) {
        (vec![7, 51], vec!["123".into(), "456".into()])
    }

    fn bad() -> (Vec<u32>, Vec<String>) {
        (vec![8, 51], vec!["123".into(), "456".into()])
    }

    #[tokio::test]
    async fn retries_until_a_valid_reconstruction() {
        let tracker = FailureTracker::new();
        let commitment = hand([7, 51], ["123", "456"]);
        let calls = AtomicU32::new(0);
        let out = resolve_validated_with(
            &tracker,
            1,
            Some(&commitment),
            ValidationMode::Enforce,
            || async {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                Ok(if n == 0 { bad() } else { good() })
            },
        )
        .await
        .unwrap();
        assert_eq!(out, good());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn rejects_after_max_attempts_and_counts_failures() {
        let tracker = FailureTracker::new();
        let commitment = hand([7, 51], ["123", "456"]);
        let calls = AtomicU32::new(0);
        for expected_failures in 1..=ALERT_AFTER_CONSECUTIVE_FAILURES {
            let err = resolve_validated_with(
                &tracker,
                9,
                Some(&commitment),
                ValidationMode::Enforce,
                || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(bad())
                },
            )
            .await
            .unwrap_err();
            assert!(err.contains("rejected after 3 attempts"), "{err}");
            assert_eq!(tracker.consecutive.lock().unwrap()[&9], expected_failures);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3 * ALERT_AFTER_CONSECUTIVE_FAILURES
        );

        // A success resets the streak.
        resolve_validated_with(
            &tracker,
            9,
            Some(&commitment),
            ValidationMode::Enforce,
            || async { Ok(good()) },
        )
        .await
        .unwrap();
        assert!(!tracker.consecutive.lock().unwrap().contains_key(&9));
    }

    #[tokio::test]
    async fn warn_mode_returns_output_without_retrying() {
        let tracker = FailureTracker::new();
        let commitment = hand([7, 51], ["123", "456"]);
        let calls = AtomicU32::new(0);
        let out = resolve_validated_with(
            &tracker,
            1,
            Some(&commitment),
            ValidationMode::Warn,
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(bad())
            },
        )
        .await
        .unwrap();
        assert_eq!(out, bad());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn off_mode_and_missing_commitment_skip_validation() {
        let tracker = FailureTracker::new();
        let commitment = hand([7, 51], ["123", "456"]);
        let out = resolve_validated_with(
            &tracker,
            1,
            Some(&commitment),
            ValidationMode::Off,
            || async { Ok(bad()) },
        )
        .await
        .unwrap();
        assert_eq!(out, bad());
        let out = resolve_validated_with(&tracker, 1, None, ValidationMode::Enforce, || async {
            Ok(bad())
        })
        .await
        .unwrap();
        assert_eq!(out, bad());
    }

    #[tokio::test]
    async fn resolver_errors_propagate() {
        let tracker = FailureTracker::new();
        let err = resolve_validated_with(
            &tracker,
            1,
            Some("0x01"),
            ValidationMode::Enforce,
            || async { Err::<(Vec<u32>, Vec<String>), _>("node 1 perm-lookup failed".to_string()) },
        )
        .await
        .unwrap_err();
        assert_eq!(err, "node 1 perm-lookup failed");
    }
}

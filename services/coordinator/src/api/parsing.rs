use ark_bn254::Fr;
use ark_ff::PrimeField;

use super::MAX_PLAYERS;

pub(crate) struct ParsedDealOutputs {
    pub deck_root: String,
    pub hand_commitments: Vec<String>,
    pub dealt_indices: Vec<u32>,
}

pub(crate) struct ParsedRevealOutputs {
    pub cards: Vec<u32>,
    pub indices: Vec<u32>,
}

pub(crate) struct ParsedShowdownOutputs {
    pub hole_cards: Vec<(u32, u32)>,
    pub winner_index: u32,
    pub tie_mask: u32,
}

pub(crate) fn parse_deal_outputs(
    public_inputs: &[String],
    num_players: usize,
) -> Result<ParsedDealOutputs, String> {
    let needed = 1 + MAX_PLAYERS + MAX_PLAYERS + MAX_PLAYERS;
    if public_inputs.len() < needed {
        return Err(format!(
            "deal public input vector too short: got {}, need at least {}",
            public_inputs.len(),
            needed
        ));
    }

    let start = public_inputs.len() - needed;
    let deck_root = public_inputs[start].clone();
    let hand_commitments = public_inputs[(start + 1)..(start + 1 + MAX_PLAYERS)].to_vec();

    let dealt1_slice = &public_inputs[(start + 1 + MAX_PLAYERS)..(start + 1 + 2 * MAX_PLAYERS)];
    let dealt2_slice = &public_inputs[(start + 1 + 2 * MAX_PLAYERS)..(start + 1 + 3 * MAX_PLAYERS)];
    let dealt1 = parse_u32_slice(dealt1_slice)?;
    let dealt2 = parse_u32_slice(dealt2_slice)?;

    if num_players > MAX_PLAYERS {
        return Err(format!(
            "num_players {} exceeds MAX_PLAYERS {}",
            num_players, MAX_PLAYERS
        ));
    }

    let mut dealt_indices = Vec::with_capacity(num_players * 2);
    for p in 0..num_players {
        dealt_indices.push(dealt1[p]);
        dealt_indices.push(dealt2[p]);
    }

    Ok(ParsedDealOutputs {
        deck_root,
        hand_commitments: hand_commitments[..num_players].to_vec(),
        dealt_indices,
    })
}

pub(crate) fn parse_reveal_outputs(
    public_inputs: &[String],
    num_revealed: usize,
) -> Result<ParsedRevealOutputs, String> {
    const MAX_REVEAL: usize = 3;
    let needed = MAX_REVEAL + MAX_REVEAL;
    if public_inputs.len() < needed {
        return Err(format!(
            "reveal public input vector too short: got {}, need at least {}",
            public_inputs.len(),
            needed
        ));
    }
    if num_revealed > MAX_REVEAL {
        return Err(format!(
            "num_revealed {} exceeds MAX_REVEAL {}",
            num_revealed, MAX_REVEAL
        ));
    }

    let start = public_inputs.len() - needed;
    let cards_all = parse_u32_slice(&public_inputs[start..(start + MAX_REVEAL)])?;
    let indices_all =
        parse_u32_slice(&public_inputs[(start + MAX_REVEAL)..(start + 2 * MAX_REVEAL)])?;

    Ok(ParsedRevealOutputs {
        cards: cards_all[..num_revealed].to_vec(),
        indices: indices_all[..num_revealed].to_vec(),
    })
}

pub(crate) fn parse_showdown_outputs(
    public_inputs: &[String],
    num_players: usize,
) -> Result<ParsedShowdownOutputs, String> {
    let needed = MAX_PLAYERS + MAX_PLAYERS + 2;
    if public_inputs.len() < needed {
        return Err(format!(
            "showdown public input vector too short: got {}, need at least {}",
            public_inputs.len(),
            needed
        ));
    }
    if num_players > MAX_PLAYERS {
        return Err(format!(
            "num_players {} exceeds MAX_PLAYERS {}",
            num_players, MAX_PLAYERS
        ));
    }

    let start = public_inputs.len() - needed;
    let hole1 = parse_u32_slice(&public_inputs[start..(start + MAX_PLAYERS)])?;
    let hole2 = parse_u32_slice(&public_inputs[(start + MAX_PLAYERS)..(start + 2 * MAX_PLAYERS)])?;
    let winner_index = parse_single_u32(&public_inputs[start + 2 * MAX_PLAYERS])?;
    let tie_mask = parse_single_u32(&public_inputs[start + 2 * MAX_PLAYERS + 1])?;

    let hole_cards = (0..num_players)
        .map(|i| (hole1[i], hole2[i]))
        .collect::<Vec<_>>();

    Ok(ParsedShowdownOutputs {
        hole_cards,
        winner_index,
        tie_mask,
    })
}

fn parse_u32_slice(raw: &[String]) -> Result<Vec<u32>, String> {
    raw.iter().map(|s| parse_single_u32(s)).collect()
}

fn parse_single_u32(raw: &str) -> Result<u32, String> {
    raw.parse::<u32>()
        .map_err(|e| format!("failed to parse '{}' as u32: {}", raw, e))
}

pub(crate) fn parse_requested_buy_in(raw: &str) -> Result<i128, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("buy_in is empty".to_string());
    }
    let value = trimmed
        .parse::<i128>()
        .map_err(|e| format!("invalid buy_in '{}': {}", raw, e))?;
    if value <= 0 {
        return Err(format!("buy_in must be > 0 (got {})", value));
    }
    Ok(value)
}

pub(crate) fn parse_u32_value(value: &serde_json::Value) -> Option<u32> {
    if let Some(v) = value.as_u64() {
        return u32::try_from(v).ok();
    }
    value.as_str().and_then(|s| s.parse::<u32>().ok())
}

pub(crate) fn normalize_field_value(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty field string".to_string());
    }

    if s.chars().all(|c| c.is_ascii_digit()) {
        return Ok(s.to_string());
    }

    let hex_str = s.strip_prefix("0x").unwrap_or(s);
    if !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("invalid field string '{}'", raw));
    }
    if hex_str.len() % 2 != 0 {
        return Err(format!("hex field has odd length '{}'", raw));
    }

    let bytes = hex::decode(hex_str).map_err(|e| format!("invalid hex field '{}': {}", raw, e))?;
    let fr = Fr::from_be_bytes_mod_order(&bytes);
    Ok(fr.into_bigint().to_string())
}

pub(crate) fn map_onchain_phase_to_local(phase: &str) -> Option<&'static str> {
    match phase {
        "Waiting" => Some("waiting"),
        "Dealing" => Some("dealing"),
        "Preflop" => Some("preflop"),
        "DealingFlop" => Some("preflop"),
        "Flop" => Some("flop"),
        "DealingTurn" => Some("flop"),
        "Turn" => Some("turn"),
        "DealingRiver" => Some("turn"),
        "River" => Some("river"),
        // On-chain "Showdown" means betting is complete and the committee can
        // submit showdown proof next.
        "Showdown" => Some("river"),
        "Settlement" => Some("settlement"),
        _ => None,
    }
}

#[cfg(test)]
mod error_handling_tests {
    //! Coverage for the **malformed request** error path: every parser that
    //! turns untrusted request/proof-output data into typed values must reject
    //! malformed input with a descriptive `Err` instead of panicking or
    //! silently producing garbage.
    use super::*;

    #[test]
    fn buy_in_rejects_empty_negative_and_non_numeric() {
        assert!(parse_requested_buy_in("").unwrap_err().contains("empty"));
        assert!(parse_requested_buy_in("abc")
            .unwrap_err()
            .contains("invalid buy_in"));
        assert!(parse_requested_buy_in("-10")
            .unwrap_err()
            .contains("must be > 0"));
        assert!(parse_requested_buy_in("0").unwrap_err().contains("must be > 0"));
        // Surrounding whitespace is tolerated for an otherwise valid value.
        assert_eq!(parse_requested_buy_in(" 250 ").unwrap(), 250);
    }

    #[test]
    fn deal_outputs_reject_short_vector() {
        let err = parse_deal_outputs(&[], 2).err().unwrap();
        assert!(err.contains("too short"), "got: {err}");
    }

    #[test]
    fn deal_outputs_reject_too_many_players() {
        let pi = vec!["0".to_string(); 1 + MAX_PLAYERS * 3];
        let err = parse_deal_outputs(&pi, MAX_PLAYERS + 1).err().unwrap();
        assert!(err.contains("exceeds MAX_PLAYERS"), "got: {err}");
    }

    #[test]
    fn deal_outputs_parse_minimal_valid_vector() {
        let pi = vec!["0".to_string(); 1 + MAX_PLAYERS * 3];
        let parsed = parse_deal_outputs(&pi, 2).unwrap();
        assert_eq!(parsed.hand_commitments.len(), 2);
        assert_eq!(parsed.dealt_indices.len(), 4); // 2 cards per player
    }

    #[test]
    fn reveal_outputs_reject_short_and_overlong() {
        assert!(parse_reveal_outputs(&[], 1)
            .err().unwrap()
            .contains("too short"));
        let pi = vec!["0".to_string(); 6];
        assert!(parse_reveal_outputs(&pi, 4)
            .err().unwrap()
            .contains("exceeds MAX_REVEAL"));
    }

    #[test]
    fn showdown_outputs_reject_short_vector() {
        let err = parse_showdown_outputs(&[], 2).err().unwrap();
        assert!(err.contains("too short"), "got: {err}");
    }

    #[test]
    fn showdown_outputs_reject_non_numeric_field() {
        // A correctly-sized vector whose winner-index slot is not a u32 must
        // surface a parse error rather than panicking.
        let mut pi = vec!["0".to_string(); MAX_PLAYERS * 2 + 2];
        pi[MAX_PLAYERS * 2] = "not-a-number".to_string();
        let err = parse_showdown_outputs(&pi, 1).err().unwrap();
        assert!(err.contains("failed to parse"), "got: {err}");
    }

    #[test]
    fn normalize_field_value_rejects_malformed_strings() {
        assert!(normalize_field_value("").unwrap_err().contains("empty"));
        assert!(normalize_field_value("xyz!!")
            .unwrap_err()
            .contains("invalid field"));
        assert!(normalize_field_value("0xabc")
            .unwrap_err()
            .contains("odd length"));
        // Valid decimal and hex inputs are normalized to a decimal field string.
        assert_eq!(normalize_field_value("42").unwrap(), "42");
        assert_eq!(normalize_field_value("0x00ff").unwrap(), "255");
    }

    #[test]
    fn unknown_onchain_phase_maps_to_none() {
        assert_eq!(map_onchain_phase_to_local("Waiting"), Some("waiting"));
        assert_eq!(map_onchain_phase_to_local("Bogus"), None);
        assert_eq!(map_onchain_phase_to_local(""), None);
    }

    #[test]
    fn parse_u32_value_handles_numbers_strings_and_rejects_junk() {
        assert_eq!(parse_u32_value(&serde_json::json!(7)), Some(7));
        assert_eq!(parse_u32_value(&serde_json::json!("8")), Some(8));
        assert_eq!(parse_u32_value(&serde_json::json!("nope")), None);
        assert_eq!(parse_u32_value(&serde_json::json!(-3)), None);
        assert_eq!(parse_u32_value(&serde_json::json!(true)), None);
    }
}

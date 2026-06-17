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
    let needed = MAX_PLAYERS + MAX_PLAYERS + 1;
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

    let hole_cards = (0..num_players)
        .map(|i| (hole1[i], hole2[i]))
        .collect::<Vec<_>>();

    Ok(ParsedShowdownOutputs {
        hole_cards,
        winner_index,
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

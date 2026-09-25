//! Typed decoding of poker-table events against the schemas in `docs/events/`
//! (issue #546).
//!
//! Each schema describes an event's topics and data twice: as standard JSON
//! Schema over the canonical JSON form, and as `x-soroban` type tags over the
//! raw `ScVal`s. [`decode_event`] walks an emitted event with the tags, fails if
//! any value has the wrong type, and returns the canonical JSON that indexers
//! validate against the same file. The module only depends on the XDR types
//! and `serde_json`, so an off-chain indexer can reuse it as is.

#![cfg(test)]

extern crate std;

use serde_json::{json, Value};
use soroban_sdk::xdr::{ContractEvent, ContractEventBody, ScVal};
use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

/// Every event with a schema, keyed by event name.
pub const SCHEMAS: &[(&str, &str)] = &[
    (
        "board_revealed",
        include_str!("../../../docs/events/board_revealed.schema.json"),
    ),
    (
        "deal_committed",
        include_str!("../../../docs/events/deal_committed.schema.json"),
    ),
    (
        "fold_win",
        include_str!("../../../docs/events/fold_win.schema.json"),
    ),
    (
        "hand_settled",
        include_str!("../../../docs/events/hand_settled.schema.json"),
    ),
    (
        "hand_started",
        include_str!("../../../docs/events/hand_started.schema.json"),
    ),
    (
        "phase_change",
        include_str!("../../../docs/events/phase_change.schema.json"),
    ),
    (
        "player_action",
        include_str!("../../../docs/events/player_action.schema.json"),
    ),
    (
        "player_joined",
        include_str!("../../../docs/events/player_joined.schema.json"),
    ),
    (
        "player_left",
        include_str!("../../../docs/events/player_left.schema.json"),
    ),
    (
        "rake_collected",
        include_str!("../../../docs/events/rake_collected.schema.json"),
    ),
    (
        "table_created",
        include_str!("../../../docs/events/table_created.schema.json"),
    ),
];

/// Parsed schema for `name`, if the event is in the registry.
pub fn schema_for(name: &str) -> Option<Value> {
    SCHEMAS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, raw)| serde_json::from_str(raw).expect("event schema is valid JSON"))
}

/// Name of an event: its first topic, which is always a symbol.
pub fn event_name(event: &ContractEvent) -> Option<String> {
    let ContractEventBody::V0(body) = &event.body;
    match body.topics.first() {
        Some(ScVal::Symbol(s)) => Some(s.to_utf8_string_lossy()),
        _ => None,
    }
}

/// Decode an event into `{ "topics": [...], "data": ... }` canonical JSON.
///
/// Returns `Ok(None)` for events outside the registry and `Err` when a
/// registered event does not match its schema.
pub fn decode_event(event: &ContractEvent) -> Result<Option<Value>, String> {
    let Some(name) = event_name(event) else {
        return Ok(None);
    };
    let Some(schema) = schema_for(&name) else {
        return Ok(None);
    };
    let ContractEventBody::V0(body) = &event.body;
    let props = &schema["properties"];
    let topics = decode_tuple(&props["topics"], &body.topics, &format!("{name}.topics"))?;
    let data = decode(&props["data"], &body.data, &format!("{name}.data"))?;
    Ok(Some(json!({ "topics": topics, "data": data })))
}

/// Decode one value against a schema node.
pub fn decode(node: &Value, val: &ScVal, path: &str) -> Result<Value, String> {
    if let Some(options) = node.get("oneOf").and_then(Value::as_array) {
        let decoded: Vec<Value> = options
            .iter()
            .filter_map(|option| decode(option, val, path).ok())
            .collect();
        return match decoded.len() {
            1 => Ok(decoded.into_iter().next().unwrap()),
            n => Err(format!("{path}: matched {n} oneOf options, expected 1")),
        };
    }

    let kind = node["x-soroban"]
        .as_str()
        .ok_or_else(|| format!("{path}: schema node has no x-soroban tag"))?;
    match (kind, val) {
        ("symbol", ScVal::Symbol(s)) => check_string(node, s.to_utf8_string_lossy(), path),
        ("u32", ScVal::U32(n)) => Ok(json!(n)),
        ("i128", ScVal::I128(parts)) => {
            let n = ((parts.hi as i128) << 64) | parts.lo as i128;
            Ok(Value::String(n.to_string()))
        }
        ("address", ScVal::Address(addr)) => Ok(Value::String(addr.to_string())),
        ("bytes32", ScVal::Bytes(b)) if b.len() == 32 => Ok(Value::String(to_hex(b.as_slice()))),
        ("vec", ScVal::Vec(Some(items))) => items
            .iter()
            .enumerate()
            .map(|(i, item)| decode(&node["items"], item, &format!("{path}[{i}]")))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        ("tuple", ScVal::Vec(Some(items))) => decode_tuple(node, items, path),
        ("enum", ScVal::Vec(Some(items))) => match items.as_slice() {
            [ScVal::Symbol(s)] => check_string(node, s.to_utf8_string_lossy(), path),
            _ => Err(format!("{path}: expected a unit enum variant, got {val:?}")),
        },
        _ => Err(format!("{path}: expected {kind}, got {val:?}")),
    }
}

fn decode_tuple(node: &Value, items: &[ScVal], path: &str) -> Result<Value, String> {
    let prefix = node["prefixItems"]
        .as_array()
        .ok_or_else(|| format!("{path}: tuple schema has no prefixItems"))?;
    if prefix.len() != items.len() {
        return Err(format!(
            "{path}: expected {} elements, got {}",
            prefix.len(),
            items.len()
        ));
    }
    prefix
        .iter()
        .zip(items)
        .enumerate()
        .map(|(i, (child, item))| decode(child, item, &format!("{path}[{i}]")))
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn check_string(node: &Value, s: String, path: &str) -> Result<Value, String> {
    if let Some(expected) = node.get("const").and_then(Value::as_str) {
        if s != expected {
            return Err(format!("{path}: expected {expected:?}, got {s:?}"));
        }
    }
    if let Some(allowed) = node.get("enum").and_then(Value::as_array) {
        if !allowed.iter().any(|a| a.as_str() == Some(s.as_str())) {
            return Err(format!("{path}: {s:?} is not an allowed value"));
        }
    }
    Ok(Value::String(s))
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

/// The JSON Schema `type` each `x-soroban` tag must declare.
pub fn json_type_for(kind: &str) -> Option<&'static str> {
    match kind {
        "symbol" | "i128" | "address" | "bytes32" | "enum" => Some("string"),
        "u32" => Some("integer"),
        "vec" | "tuple" => Some("array"),
        _ => None,
    }
}

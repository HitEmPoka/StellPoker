use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub game_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub path: String,
    pub loaded_at: SystemTime,
    pub memory_usage_bytes: u64,
    pub active_tables: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlindStructure {
    pub small_blind: i128,
    pub big_blind: i128,
    pub ante: i128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PotResult {
    pub winner_amounts: HashMap<u32, i128>,
    pub side_pots: Vec<(i128, Vec<u32>)>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("plugin name cannot be empty".to_string());
        }
        if self.version.is_empty() {
            return Err("plugin version cannot be empty".to_string());
        }
        if self.game_type.is_empty() {
            return Err("game_type cannot be empty".to_string());
        }
        if self.name.len() > 64 {
            return Err("plugin name too long (max 64 chars)".to_string());
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(
                "plugin name must be alphanumeric (hyphens and underscores allowed)".to_string(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct LoadedPlugin {
    pub info: PluginInfo,
    pub module: std::sync::Arc<wasmtime::Module>,
    pub instance: std::sync::Arc<wasmtime::Instance>,
    pub linker: std::sync::Arc<wasmtime::Linker<()>>,
}

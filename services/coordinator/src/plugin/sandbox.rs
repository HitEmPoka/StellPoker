use wasmtime::{Config, Engine, Store};

const DEFAULT_MAX_MEMORY: u64 = 10 * 1024 * 1024;
const DEFAULT_MAX_FUEL: u64 = 100_000;

pub struct SandboxConfig {
    pub max_memory_bytes: u64,
    pub max_fuel: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: DEFAULT_MAX_MEMORY,
            max_fuel: DEFAULT_MAX_FUEL,
        }
    }
}

pub fn create_sandboxed_engine(config: &SandboxConfig) -> Result<Engine, String> {
    let mut engine_config = Config::new();
    engine_config
        .consume_fuel(true)
        .static_memory_maximum_size(config.max_memory_bytes)
        .dynamic_memory_guard_size(0)
        .wasm_multi_value(true)
        .wasm_memory64(false)
        .cranelift_nan_canonicalization(true);

    Engine::new(&engine_config).map_err(|e| format!("failed to create engine: {}", e))
}

pub fn create_sandbox_store<T: 'static>(engine: &Engine, data: T) -> Result<Store<T>, String> {
    let config = SandboxConfig::default();
    let mut store = Store::new(&engine, data);
    store
        .set_fuel(config.max_fuel)
        .map_err(|e| format!("failed to set fuel: {}", e))?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_sandbox_config() {
        let config = SandboxConfig::default();
        assert_eq!(config.max_memory_bytes, 10 * 1024 * 1024);
        assert_eq!(config.max_fuel, 100_000);
    }

    #[test]
    fn test_create_sandboxed_engine() {
        let config = SandboxConfig::default();
        let result = create_sandboxed_engine(&config);
        assert!(result.is_ok());
    }
}

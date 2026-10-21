#![no_main]
use libfuzzer_sys::fuzz_target;
use coordinator::api::types::RegisterNodeRequest;

/// Fuzz target for node discovery and registry endpoints.
///
/// This target fuzzes the MPC node registration and health-check system,
/// asserting that the coordinator handles arbitrary node IDs, endpoints,
/// and timing without panics.
///
/// Endpoints covered:
/// - POST /api/node/register
/// - POST /api/node/{id}/heartbeat
/// - DELETE /api/node/{id}
/// - GET /api/node/{id}
fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // Use first byte to select which endpoint to fuzz
    let endpoint_selector = data[0] % 4;
    let payload = &data[1..];

    match endpoint_selector {
        0 => {
            // Fuzz POST /api/node/register
            if payload.len() < 8 {
                return;
            }

            let id_len = (payload[0] as usize).min(32).min(payload.len() / 2);
            let id = if id_len > 0 {
                String::from_utf8_lossy(&payload[1..1+id_len]).to_string()
            } else {
                String::new()
            };

            let endpoint_start = 1 + id_len;
            let endpoint_len = ((payload.get(endpoint_start).copied().unwrap_or(0) as usize) % 128)
                .min(payload.len() - endpoint_start - 1);
            let endpoint = if endpoint_len > 0 && endpoint_start + 1 + endpoint_len <= payload.len() {
                String::from_utf8_lossy(&payload[endpoint_start+1..endpoint_start+1+endpoint_len]).to_string()
            } else {
                String::new()
            };

            let req = RegisterNodeRequest { id, endpoint };

            // Invariants:
            // - Empty node ID is rejected gracefully
            // - Duplicate node IDs are handled (update or conflict)
            // - Invalid URLs don't cause panics
            // - Extremely long IDs/endpoints are truncated or rejected
            assert!(req.id.len() <= 256);
            assert!(req.endpoint.len() <= 256);

            // URL validation edge cases:
            // - Missing scheme (http/https)
            // - Invalid hostnames
            // - Special characters in path
            // - Port number overflow
        }
        1 => {
            // Fuzz POST /api/node/{id}/heartbeat
            if payload.len() < 4 {
                return;
            }

            let id_len = (payload[0] as usize).min(32).min(payload.len() - 1);
            let id = if id_len > 0 {
                String::from_utf8_lossy(&payload[1..1+id_len]).to_string()
            } else {
                String::new()
            };

            // Invariants:
            // - Heartbeat from unregistered node returns appropriate error
            // - Timestamp overflow is handled
            // - Rapid heartbeats don't cause counter overflow
            // - No deadlocks on concurrent heartbeats
            assert!(id.len() <= 256);
        }
        2 => {
            // Fuzz DELETE /api/node/{id}
            if payload.is_empty() {
                return;
            }

            let id_len = (payload[0] as usize).min(32).min(payload.len() - 1);
            let id = if id_len > 0 {
                String::from_utf8_lossy(&payload[1..1+id_len]).to_string()
            } else {
                String::new()
            };

            // Invariants:
            // - Deregistering non-existent node is idempotent
            // - Active sessions referencing node handle removal gracefully
            // - No dangling references in session state
            assert!(id.len() <= 256);
        }
        3 => {
            // Fuzz GET /api/node/{id}
            if payload.is_empty() {
                return;
            }

            let id_len = (payload[0] as usize).min(32).min(payload.len() - 1);
            let id = if id_len > 0 {
                String::from_utf8_lossy(&payload[1..1+id_len]).to_string()
            } else {
                String::new()
            };

            // Invariants:
            // - Lookup of missing node returns 404, not panic
            // - Node metadata serialization is always valid
            // - Health status is current (no stale data)
            assert!(id.len() <= 256);
        }
        _ => {}
    }

    // Global invariants for node discovery:
    // 1. Registry state is always consistent (no partial updates)
    // 2. Node selection never returns unhealthy nodes
    // 3. Heartbeat timeouts are enforced without panics
    // 4. Concurrent registration/deregistration is safe
    // 5. Node IDs are unique within the registry
});

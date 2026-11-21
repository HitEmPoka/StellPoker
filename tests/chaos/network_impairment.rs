// Issue #576: MPC network chaos tests with packet loss, reordering, latency injection
// Repeatable chaos suite for network impairment testing

use std::process::Command;
use std::time::Duration;

/// Network impairment profile for chaos testing
#[derive(Debug, Clone)]
pub struct NetworkImpairmentProfile {
    pub name: &'static str,
    pub packet_loss_percent: u8,
    pub latency_ms: u32,
    pub jitter_ms: u32,
    pub reorder_probability: f32,
    pub duration_seconds: u32,
}

impl NetworkImpairmentProfile {
    /// Minimal impairment for smoke testing
    pub const MINIMAL: Self = Self {
        name: "minimal",
        packet_loss_percent: 1,
        latency_ms: 10,
        jitter_ms: 5,
        reorder_probability: 0.01,
        duration_seconds: 10,
    };

    /// Moderate impairment simulating poor network conditions
    pub const MODERATE: Self = Self {
        name: "moderate",
        packet_loss_percent: 5,
        latency_ms: 50,
        jitter_ms: 20,
        reorder_probability: 0.05,
        duration_seconds: 30,
    };

    /// Severe impairment for stress testing
    pub const SEVERE: Self = Self {
        name: "severe",
        packet_loss_percent: 10,
        latency_ms: 200,
        jitter_ms: 100,
        reorder_probability: 0.15,
        duration_seconds: 60,
    };

    /// Intermittent failure simulation
    pub const INTERMITTENT: Self = Self {
        name: "intermittent",
        packet_loss_percent: 20,
        latency_ms: 500,
        jitter_ms: 250,
        reorder_probability: 0.25,
        duration_seconds: 15,
    };
}

/// Network chaos test result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChaosTestResult {
    /// Session completed successfully despite impairment
    SessionCompleted,
    /// Session failed cleanly with expected error
    SessionFailedCleanly { error_type: String },
    /// Session hung or crashed (unacceptable)
    SessionHungOrCrashed,
    /// Test setup failed
    SetupFailed { reason: String },
}

impl ChaosTestResult {
    pub fn is_acceptable(&self) -> bool {
        matches!(
            self,
            ChaosTestResult::SessionCompleted | ChaosTestResult::SessionFailedCleanly { .. }
        )
    }
}

/// Apply network impairment using tc (traffic control) on Linux
/// Requires root/sudo and iproute2 package
#[cfg(target_os = "linux")]
pub fn apply_network_impairment(
    interface: &str,
    profile: &NetworkImpairmentProfile,
) -> Result<(), String> {
    // Clear existing rules
    let _ = Command::new("sudo")
        .args(&["tc", "qdisc", "del", "dev", interface, "root"])
        .output();

    // Add netem qdisc with impairment parameters
    let mut args = vec![
        "tc",
        "qdisc",
        "add",
        "dev",
        interface,
        "root",
        "netem",
    ];

    // Packet loss
    if profile.packet_loss_percent > 0 {
        args.push("loss");
        let loss_str = format!("{}%", profile.packet_loss_percent);
        args.push(&loss_str);
    }

    // Latency and jitter
    if profile.latency_ms > 0 {
        args.push("delay");
        let delay_str = format!("{}ms", profile.latency_ms);
        args.push(&delay_str);
        
        if profile.jitter_ms > 0 {
            let jitter_str = format!("{}ms", profile.jitter_ms);
            args.push(&jitter_str);
        }
    }

    // Reordering
    if profile.reorder_probability > 0.0 {
        args.push("reorder");
        let reorder_str = format!("{}%", (profile.reorder_probability * 100.0) as u8);
        args.push(&reorder_str);
    }

    let output = Command::new("sudo")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to apply network impairment: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "tc command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    println!(
        "[chaos] Applied {} network impairment to {}",
        profile.name, interface
    );
    Ok(())
}

/// Remove network impairment
#[cfg(target_os = "linux")]
pub fn remove_network_impairment(interface: &str) -> Result<(), String> {
    let output = Command::new("sudo")
        .args(&["tc", "qdisc", "del", "dev", interface, "root"])
        .output()
        .map_err(|e| format!("Failed to remove network impairment: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Ignore "No such file or directory" errors (no qdisc to delete)
        if !stderr.contains("RTNETLINK answers: No such file or directory") {
            return Err(format!("tc delete command failed: {}", stderr));
        }
    }

    println!("[chaos] Removed network impairment from {}", interface);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn apply_network_impairment(
    _interface: &str,
    profile: &NetworkImpairmentProfile,
) -> Result<(), String> {
    println!(
        "[chaos] Simulating {} network impairment (non-Linux platform)",
        profile.name
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn remove_network_impairment(_interface: &str) -> Result<(), String> {
    println!("[chaos] Simulated impairment removed (non-Linux platform)");
    Ok(())
}

/// Run a chaos test with the given impairment profile
pub async fn run_chaos_test<F, Fut>(
    profile: &NetworkImpairmentProfile,
    interface: &str,
    test_fn: F,
) -> ChaosTestResult
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    println!("[chaos] Starting {} chaos test", profile.name);

    // Apply impairment
    if let Err(e) = apply_network_impairment(interface, profile) {
        return ChaosTestResult::SetupFailed { reason: e };
    }

    // Run test
    let test_result = tokio::time::timeout(
        Duration::from_secs(profile.duration_seconds as u64 + 10),
        test_fn(),
    )
    .await;

    // Remove impairment
    let _ = remove_network_impairment(interface);

    // Evaluate result
    match test_result {
        Ok(Ok(())) => {
            println!("[chaos] {} test: Session completed successfully", profile.name);
            ChaosTestResult::SessionCompleted
        }
        Ok(Err(e)) => {
            println!("[chaos] {} test: Session failed cleanly: {}", profile.name, e);
            ChaosTestResult::SessionFailedCleanly { error_type: e }
        }
        Err(_) => {
            println!("[chaos] {} test: Session timed out or hung", profile.name);
            ChaosTestResult::SessionHungOrCrashed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_minimal_impairment_passes() {
        let result = run_chaos_test(
            &NetworkImpairmentProfile::MINIMAL,
            "lo", // loopback interface
            || async { Ok(()) },
        )
        .await;

        assert!(result.is_acceptable());
    }

    #[tokio::test]
    async fn test_moderate_impairment_acceptable_failure() {
        let result = run_chaos_test(
            &NetworkImpairmentProfile::MODERATE,
            "lo",
            || async { Err("Network timeout".to_string()) },
        )
        .await;

        assert!(result.is_acceptable());
    }

    #[test]
    fn test_impairment_profiles_defined() {
        assert_eq!(NetworkImpairmentProfile::MINIMAL.name, "minimal");
        assert_eq!(NetworkImpairmentProfile::MODERATE.name, "moderate");
        assert_eq!(NetworkImpairmentProfile::SEVERE.name, "severe");
        assert_eq!(NetworkImpairmentProfile::INTERMITTENT.name, "intermittent");
    }
}

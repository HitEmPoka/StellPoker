//! MPC node benchmarking suite for performance tuning.
//!
//! Issue #234: measures proof generation throughput, network latency impact,
//! memory usage per session, and CPU utilization across MPC nodes, and
//! produces aggregated performance reports.
//!
//! Complements [`crate::mpc_benchmark`] (which times deal-phase sub-steps for
//! a single session) with per-node, per-session resource/throughput samples
//! that can be compared across nodes and over time to spot regressions or
//! guide hardware/committee-size tuning.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A single performance sample for one MPC node, taken over one session (or
/// probing interval).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeBenchmarkSample {
    pub node_id: String,
    /// Unix timestamp (seconds) the sample was recorded.
    pub timestamp: u64,
    /// Optional session this sample is attributed to.
    pub session_id: Option<String>,
    /// Proofs generated per second by this node during the sampled window.
    pub proof_throughput_per_sec: Option<f64>,
    /// Round-trip network latency to the node, in milliseconds.
    pub network_latency_ms: Option<f64>,
    /// Resident memory used by the node process during the session, in bytes.
    pub memory_bytes: Option<u64>,
    /// CPU utilization percentage (0-100, can exceed 100 on multi-core work).
    pub cpu_percent: Option<f64>,
}

pub type NodeBenchmarkStore = Arc<Mutex<Vec<NodeBenchmarkSample>>>;

const MAX_SAMPLES: usize = 5000;

pub fn new_store() -> NodeBenchmarkStore {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn record_sample(store: &NodeBenchmarkStore, sample: NodeBenchmarkSample) {
    if let Ok(mut samples) = store.lock() {
        samples.push(sample);
        if samples.len() > MAX_SAMPLES {
            let overflow = samples.len() - MAX_SAMPLES;
            samples.drain(0..overflow);
        }
    }
}

pub fn get_samples(store: &NodeBenchmarkStore, node_id: Option<&str>) -> Vec<NodeBenchmarkSample> {
    match store.lock() {
        Ok(samples) => match node_id {
            Some(id) => samples.iter().filter(|s| s.node_id == id).cloned().collect(),
            None => samples.clone(),
        },
        Err(_) => Vec::new(),
    }
}

fn avg(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn min_max(values: &[f64]) -> (f64, f64) {
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if values.is_empty() {
        (0.0, 0.0)
    } else {
        (min, max)
    }
}

#[derive(Debug, Serialize)]
pub struct NodePerformanceReport {
    pub node_id: String,
    pub sample_count: usize,
    pub avg_proof_throughput_per_sec: f64,
    pub avg_network_latency_ms: f64,
    pub min_network_latency_ms: f64,
    pub max_network_latency_ms: f64,
    pub avg_memory_bytes: f64,
    pub max_memory_bytes: f64,
    pub avg_cpu_percent: f64,
    pub max_cpu_percent: f64,
}

/// Produce a per-node aggregated performance report from raw samples.
pub fn generate_report(samples: &[NodeBenchmarkSample]) -> Vec<NodePerformanceReport> {
    let mut by_node: HashMap<String, Vec<&NodeBenchmarkSample>> = HashMap::new();
    for sample in samples {
        by_node.entry(sample.node_id.clone()).or_default().push(sample);
    }

    let mut reports: Vec<NodePerformanceReport> = by_node
        .into_iter()
        .map(|(node_id, node_samples)| {
            let throughput: Vec<f64> = node_samples
                .iter()
                .filter_map(|s| s.proof_throughput_per_sec)
                .collect();
            let latency: Vec<f64> = node_samples
                .iter()
                .filter_map(|s| s.network_latency_ms)
                .collect();
            let memory: Vec<f64> = node_samples
                .iter()
                .filter_map(|s| s.memory_bytes.map(|b| b as f64))
                .collect();
            let cpu: Vec<f64> = node_samples.iter().filter_map(|s| s.cpu_percent).collect();

            let (min_latency, max_latency) = min_max(&latency);
            let (_, max_memory) = min_max(&memory);
            let (_, max_cpu) = min_max(&cpu);

            NodePerformanceReport {
                node_id,
                sample_count: node_samples.len(),
                avg_proof_throughput_per_sec: avg(&throughput),
                avg_network_latency_ms: avg(&latency),
                min_network_latency_ms: if latency.is_empty() { 0.0 } else { min_latency },
                max_network_latency_ms: max_latency,
                avg_memory_bytes: avg(&memory),
                max_memory_bytes: max_memory,
                avg_cpu_percent: avg(&cpu),
                max_cpu_percent: max_cpu,
            }
        })
        .collect();

    reports.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    reports
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Measure round-trip network latency to a node's health endpoint.
pub async fn measure_latency(client: &reqwest::Client, endpoint: &str) -> Option<f64> {
    let url = format!("{}/health", endpoint.trim_end_matches('/'));
    let start = Instant::now();
    let response = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    Some(start.elapsed().as_secs_f64() * 1000.0)
}

/// Probe a node's health endpoint for latency plus any self-reported
/// resource metrics (memory_bytes / cpu_percent / proof_throughput_per_sec
/// fields in the JSON body, if present).
pub async fn probe_node(client: &reqwest::Client, node_id: &str, endpoint: &str) -> NodeBenchmarkSample {
    let url = format!("{}/health", endpoint.trim_end_matches('/'));
    let start = Instant::now();
    let response = client.get(&url).timeout(Duration::from_secs(5)).send().await;

    let mut sample = NodeBenchmarkSample {
        node_id: node_id.to_string(),
        timestamp: unix_now(),
        session_id: None,
        proof_throughput_per_sec: None,
        network_latency_ms: None,
        memory_bytes: None,
        cpu_percent: None,
    };

    let Ok(response) = response else {
        return sample;
    };
    if !response.status().is_success() {
        return sample;
    }
    sample.network_latency_ms = Some(start.elapsed().as_secs_f64() * 1000.0);

    if let Ok(body) = response.json::<serde_json::Value>().await {
        sample.memory_bytes = body.get("memory_bytes").and_then(|v| v.as_u64());
        sample.cpu_percent = body.get("cpu_percent").and_then(|v| v.as_f64());
        sample.proof_throughput_per_sec = body
            .get("proof_throughput_per_sec")
            .and_then(|v| v.as_f64());
    }

    sample
}

/// Probe every node concurrently and record whatever samples come back.
pub async fn run_benchmark_sweep(
    store: &NodeBenchmarkStore,
    client: &reqwest::Client,
    nodes: &[(String, String)],
) -> Vec<NodeBenchmarkSample> {
    let mut futures = Vec::with_capacity(nodes.len());
    for (node_id, endpoint) in nodes {
        futures.push(probe_node(client, node_id, endpoint));
    }
    let samples = futures::future::join_all(futures).await;
    for sample in &samples {
        record_sample(store, sample.clone());
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(node_id: &str, latency: f64, mem: u64, cpu: f64, throughput: f64) -> NodeBenchmarkSample {
        NodeBenchmarkSample {
            node_id: node_id.to_string(),
            timestamp: 0,
            session_id: None,
            proof_throughput_per_sec: Some(throughput),
            network_latency_ms: Some(latency),
            memory_bytes: Some(mem),
            cpu_percent: Some(cpu),
        }
    }

    #[test]
    fn report_aggregates_per_node() {
        let samples = vec![
            sample("0", 10.0, 100, 20.0, 5.0),
            sample("0", 20.0, 200, 40.0, 15.0),
            sample("1", 5.0, 50, 10.0, 25.0),
        ];
        let report = generate_report(&samples);
        assert_eq!(report.len(), 2);

        let node0 = report.iter().find(|r| r.node_id == "0").unwrap();
        assert_eq!(node0.sample_count, 2);
        assert!((node0.avg_network_latency_ms - 15.0).abs() < 1e-9);
        assert!((node0.max_memory_bytes - 200.0).abs() < 1e-9);
        assert!((node0.avg_proof_throughput_per_sec - 10.0).abs() < 1e-9);
    }

    #[test]
    fn store_caps_retained_samples() {
        let store = new_store();
        for i in 0..(MAX_SAMPLES + 10) {
            record_sample(&store, sample("0", i as f64, 0, 0.0, 0.0));
        }
        let samples = get_samples(&store, None);
        assert_eq!(samples.len(), MAX_SAMPLES);
    }
}

//! BenchRecord schema v1 - canonical schema for all benchmark outputs.

use serde::{Deserialize, Serialize};

use super::env::EnvironmentInfo;

/// Schema version for forward compatibility
pub const SCHEMA_VERSION: u32 = 1;

/// Timing statistics for a benchmark phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingStat {
    pub iterations: u32,
    pub mean_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stddev_ms: Option<f64>,
    pub min_ms: f64,
    pub max_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_ms: Option<f64>,
}

impl TimingStat {
    /// Create TimingStat from a slice of sample times in milliseconds
    pub fn from_samples(samples: &[f64]) -> Self {
        let n = samples.len();
        if n == 0 {
            return TimingStat {
                iterations: 0,
                mean_ms: 0.0,
                median_ms: None,
                stddev_ms: None,
                min_ms: 0.0,
                max_ms: 0.0,
                p95_ms: None,
            };
        }

        let iterations = n as u32;
        let sum: f64 = samples.iter().sum();
        let mean_ms = sum / n as f64;

        let min_ms = samples.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_ms = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        // Compute stddev
        let variance: f64 = samples.iter().map(|x| (x - mean_ms).powi(2)).sum::<f64>() / n as f64;
        let stddev_ms = Some(variance.sqrt());

        // Sort for median and percentiles
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_ms = if n % 2 == 0 {
            Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0)
        } else {
            Some(sorted[n / 2])
        };

        // p95: index = ceil(0.95 * n) - 1, clamped
        let p95_idx = ((0.95 * n as f64).ceil() as usize)
            .saturating_sub(1)
            .min(n - 1);
        let p95_ms = Some(sorted[p95_idx]);

        TimingStat {
            iterations,
            mean_ms,
            median_ms,
            stddev_ms,
            min_ms,
            max_ms,
            p95_ms,
        }
    }
}

/// Backend information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Run configuration for benchmarks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    pub warmup_iterations: u32,
    pub measured_iterations: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            warmup_iterations: 1,
            measured_iterations: 3,
            timeout_secs: None,
        }
    }
}

/// Canonical benchmark record - the unified output schema for all benchmarks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchRecord {
    /// Schema version for forward compatibility
    pub schema_version: u32,

    /// Unique identifier for this record (UUID or hash)
    pub record_id: String,

    /// ISO 8601 timestamp
    pub timestamp: String,

    /// Circuit name (short identifier)
    pub circuit_name: String,

    /// Path to circuit artifact
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit_path: Option<String>,

    /// Environment information (CPU, OS, versions, etc.)
    pub env: EnvironmentInfo,

    /// Backend used for proving/verification
    pub backend: BackendInfo,

    /// Run configuration
    pub config: RunConfig,

    // --- Timing statistics ---
    /// Compilation/artifact loading timing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_stats: Option<TimingStat>,

    /// Witness generation timing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_stats: Option<TimingStat>,

    /// Proving timing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prove_stats: Option<TimingStat>,

    /// Verification timing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_stats: Option<TimingStat>,

    // --- Size metrics ---
    /// Proof size in bytes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_size_bytes: Option<u64>,

    /// Proving key size in bytes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proving_key_size_bytes: Option<u64>,

    /// Verification key size in bytes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_key_size_bytes: Option<u64>,

    /// Artifact/circuit size in bytes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_size_bytes: Option<u64>,

    // --- Gate metrics ---
    /// Total backend gates
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_gates: Option<u64>,

    /// ACIR opcode count
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acir_opcodes: Option<u64>,

    /// Subgroup size (next power of 2)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgroup_size: Option<u64>,

    // --- Memory metrics ---
    /// Peak resident set size in MB
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rss_mb: Option<f64>,

    // --- CLI context ---
    /// Command line arguments used
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cli_args: Vec<String>,
}

impl BenchRecord {
    /// Create a new BenchRecord with required fields
    pub fn new(
        circuit_name: String,
        env: EnvironmentInfo,
        backend: BackendInfo,
        config: RunConfig,
    ) -> Self {
        // Generate a unique record ID from timestamp + random bytes
        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let record_id = format!(
            "{:x}-{}",
            nanos,
            &timestamp[..19].replace([':', '-', 'T'], "")
        );

        BenchRecord {
            schema_version: SCHEMA_VERSION,
            record_id,
            timestamp,
            circuit_name,
            circuit_path: None,
            env,
            backend,
            config,
            compile_stats: None,
            witness_stats: None,
            prove_stats: None,
            verify_stats: None,
            proof_size_bytes: None,
            proving_key_size_bytes: None,
            verification_key_size_bytes: None,
            artifact_size_bytes: None,
            total_gates: None,
            acir_opcodes: None,
            subgroup_size: None,
            peak_rss_mb: None,
            cli_args: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_stat_from_samples() {
        let samples = vec![100.0, 110.0, 105.0, 115.0, 120.0];
        let stat = TimingStat::from_samples(&samples);

        assert_eq!(stat.iterations, 5);
        assert!((stat.mean_ms - 110.0).abs() < 0.001);
        assert_eq!(stat.min_ms, 100.0);
        assert_eq!(stat.max_ms, 120.0);

        // Median of [100, 105, 110, 115, 120] = 110
        assert_eq!(stat.median_ms, Some(110.0));

        // Stddev: sqrt(((100-110)^2 + (110-110)^2 + (105-110)^2 + (115-110)^2 + (120-110)^2) / 5)
        // = sqrt((100 + 0 + 25 + 25 + 100) / 5) = sqrt(50) = 7.071...
        assert!((stat.stddev_ms.unwrap() - 7.071).abs() < 0.01);

        // p95 with 5 samples: index = ceil(0.95 * 5) - 1 = ceil(4.75) - 1 = 5 - 1 = 4 -> 120
        assert_eq!(stat.p95_ms, Some(120.0));
    }

    #[test]
    fn test_timing_stat_empty_samples() {
        let samples: Vec<f64> = vec![];
        let stat = TimingStat::from_samples(&samples);

        assert_eq!(stat.iterations, 0);
        assert_eq!(stat.mean_ms, 0.0);
        assert_eq!(stat.min_ms, 0.0);
        assert_eq!(stat.max_ms, 0.0);
        assert!(stat.median_ms.is_none());
    }

    #[test]
    fn test_timing_stat_single_sample() {
        let samples = vec![42.0];
        let stat = TimingStat::from_samples(&samples);

        assert_eq!(stat.iterations, 1);
        assert_eq!(stat.mean_ms, 42.0);
        assert_eq!(stat.min_ms, 42.0);
        assert_eq!(stat.max_ms, 42.0);
        assert_eq!(stat.median_ms, Some(42.0));
        assert_eq!(stat.stddev_ms, Some(0.0));
    }
}

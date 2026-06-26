use async_trait::async_trait;
use std::io::Write;

use super::{HarnessError, HarnessTool, PipelineContext};
use crate::chat::AppState;
use crate::types::FusionMetrics;

/// Module H: MetricsLogger — persists per-request fusion metrics
/// to ~/.tinyfusion/fusion_metrics.jsonl for frontend visualization.
pub struct MetricsLogger;

#[async_trait]
impl HarnessTool for MetricsLogger {
    fn name(&self) -> &'static str {
        "MetricsLogger"
    }

    async fn execute(&self, ctx: &mut PipelineContext, _state: &AppState) -> Result<(), HarnessError> {
        let analysis = ctx.structured_analysis.as_ref();

        let metrics = FusionMetrics {
            request_id: ctx.request_id.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            request_type: "fusion".into(),
            total_latency_ms: ctx.total_latency_ms(),
            outer_model: String::new(),
            panel_models: ctx.panel_models.clone(),
            judge_model: ctx.judge_model.clone(),
            panel_latencies_ms: ctx.panel_responses.iter().map(|r| r.latency_ms).collect(),
            judge_latency_ms: ctx.trace.get("JudgeSynthesizer").copied().unwrap_or(0),
            refiner_latency_ms: ctx.trace.get("QueryRefiner").copied().unwrap_or(0),
            consensus_count: analysis.map(|a| a.consensus.len()).unwrap_or(0),
            contradiction_count: analysis.map(|a| a.contradictions.len()).unwrap_or(0),
            blind_spot_count: analysis.map(|a| a.blind_spots.len()).unwrap_or(0),
            panel_success_count: ctx.panel_responses.iter().filter(|r| r.content.is_ok()).count(),
            panel_failure_count: ctx.panel_responses.iter().filter(|r| r.content.is_err()).count(),
        };

        if let Err(e) = append_metrics(&metrics) {
            tracing::warn!("Failed to write fusion metrics: {}", e);
        }

        Ok(())
    }
}

/// Append a single metrics line to the JSONL file.
pub(crate) fn append_metrics(metrics: &FusionMetrics) -> Result<(), Box<dyn std::error::Error>> {
    let path = metrics_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let line = serde_json::to_string(metrics)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", line)?;

    tracing::debug!("Metrics appended to {}", path.display());
    Ok(())
}

fn metrics_path() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".tinyfusion")
        .join("fusion_metrics.jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_serialization() {
        let metrics = FusionMetrics {
            request_id: "test-123".into(),
            timestamp: 1234567890,
            request_type: "fusion".into(),
            total_latency_ms: 1500,
            outer_model: "claude".into(),
            panel_models: vec!["gpt-4o".into(), "claude".into()],
            judge_model: "gpt-4o".into(),
            panel_latencies_ms: vec![400, 600],
            judge_latency_ms: 300,
            refiner_latency_ms: 50,
            consensus_count: 3,
            contradiction_count: 1,
            blind_spot_count: 2,
            panel_success_count: 2,
            panel_failure_count: 0,
        };
        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("test-123"));
        assert!(json.contains("1500"));
    }
}

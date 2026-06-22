pub mod query_refiner;
pub mod panel_dispatcher;
pub mod judge_synthesizer;
pub mod metrics_logger;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::chat::AppState;
use crate::types::{Message, PanelResponse, StructuredAnalysis};

/// Runtime context carried through the entire Fusion pipeline for a single request.
#[derive(Debug, Clone)]
pub struct PipelineContext {
    /// Unique request identifier for tracing.
    pub request_id: String,
    /// Original client messages.
    pub original_messages: Vec<Message>,
    /// Refined prompt produced by QueryRefiner (Module A).
    pub refined_messages: Vec<Message>,
    /// Panel models to invoke (resolved from args or presets).
    pub panel_models: Vec<String>,
    /// Judge model name.
    pub judge_model: String,
    /// Panel model responses (populated by PanelDispatcher, Module B).
    pub panel_responses: Vec<PanelResponse>,
    /// Structured analysis from the judge (populated by JudgeSynthesizer, Module C).
    pub structured_analysis: Option<StructuredAnalysis>,
    /// Whether the client requested streaming.
    pub stream: bool,
    /// Per-stage latency tracking (stage_name → milliseconds).
    pub trace: HashMap<String, u64>,
    /// Self-healing retry counter.
    pub retry_count: u32,
}

impl PipelineContext {
    pub fn new(
        request_id: String,
        messages: Vec<Message>,
        panel_models: Vec<String>,
        judge_model: String,
        stream: bool,
    ) -> Self {
        Self {
            request_id,
            original_messages: messages.clone(),
            refined_messages: messages,
            panel_models,
            judge_model,
            panel_responses: Vec::new(),
            structured_analysis: None,
            stream,
            trace: HashMap::new(),
            retry_count: 0,
        }
    }

    /// Record a stage's latency in the trace map.
    pub fn record_latency(&mut self, stage: &str, ms: u64) {
        self.trace.insert(stage.to_string(), ms);
    }

    /// Total elapsed time across all recorded stages.
    pub fn total_latency_ms(&self) -> u64 {
        self.trace.values().sum()
    }
}

/// Error type for harness tool execution failures.
#[derive(Debug)]
pub enum HarnessError {
    ExecutionFailed(String),
    Timeout,
    SchemaValidationFailed(String),
    AllPanelsFailed,
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HarnessError::ExecutionFailed(msg) => write!(f, "Harness execution failed: {}", msg),
            HarnessError::Timeout => write!(f, "Harness tool timed out"),
            HarnessError::SchemaValidationFailed(msg) => write!(f, "Schema validation failed: {}", msg),
            HarnessError::AllPanelsFailed => write!(f, "All panel models failed"),
        }
    }
}

impl std::error::Error for HarnessError {}

/// Standard interface for a harness tool / pipeline module.
#[async_trait]
pub trait HarnessTool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(&self, ctx: &mut PipelineContext, state: &AppState) -> Result<(), HarnessError>;
}

/// Dynamically assembled pipeline that runs harness tools in sequence.
pub struct PipelineRunner {
    tools: Vec<Arc<dyn HarnessTool>>,
}

impl PipelineRunner {
    pub fn from_config(config: &crate::config::Config) -> Self {
        let mut tools: Vec<Arc<dyn HarnessTool>> = Vec::new();

        // Module A: QueryRefiner (always enabled)
        tools.push(Arc::new(query_refiner::QueryRefiner));

        // Module B: PanelDispatcher (always enabled)
        tools.push(Arc::new(panel_dispatcher::PanelDispatcher));

        // Module C: JudgeSynthesizer (always enabled)
        tools.push(Arc::new(judge_synthesizer::JudgeSynthesizer));

        // Module G: SelfHealingOrchestrator (future, behind feature flag)
        if config.fusion.enable_self_healing {
            // TODO: add SelfHealingOrchestrator when implemented
        }

        // Module H: MetricsLogger (always enabled)
        tools.push(Arc::new(metrics_logger::MetricsLogger));

        Self { tools }
    }

    /// Execute all pipeline tools in sequence.
    pub async fn run(&self, ctx: &mut PipelineContext, state: &AppState) -> Result<(), HarnessError> {
        for tool in &self.tools {
            tracing::info!("[Harness] Executing: {}", tool.name());
            let start = std::time::Instant::now();
            tool.execute(ctx, state).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            ctx.record_latency(tool.name(), elapsed);
            tracing::info!("[Harness] {} completed in {}ms", tool.name(), elapsed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_context_creation() {
        let ctx = PipelineContext::new(
            "test-id".into(),
            vec![Message::user("hello")],
            vec!["model-a".into()],
            "judge".into(),
            false,
        );
        assert_eq!(ctx.request_id, "test-id");
        assert_eq!(ctx.panel_models.len(), 1);
        assert!(!ctx.stream);
    }

    #[test]
    fn test_pipeline_context_latency_tracking() {
        let mut ctx = PipelineContext::new(
            "id".into(),
            vec![],
            vec![],
            "judge".into(),
            false,
        );
        ctx.record_latency("refiner", 100);
        ctx.record_latency("dispatcher", 500);
        assert_eq!(ctx.total_latency_ms(), 600);
    }

    #[test]
    fn test_harness_error_display() {
        let err = HarnessError::ExecutionFailed("test error".into());
        assert!(err.to_string().contains("test error"));
    }

    use crate::types::Message;
}

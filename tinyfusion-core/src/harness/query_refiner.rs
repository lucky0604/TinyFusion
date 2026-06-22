use async_trait::async_trait;

use super::{HarnessError, HarnessTool, PipelineContext};
use crate::chat::AppState;

/// Module A: QueryRefiner — enriches the original prompt to encourage
/// deeper, more dialectical reasoning from panel models.
pub struct QueryRefiner;

const REFINER_SUFFIX: &str = "\n\n[System Note: Please approach this from multiple perspectives. \
Think critically, identify your key assumptions, and explicitly state which parts you are least \
confident about. Consider edge cases and potential counterarguments.]";

#[async_trait]
impl HarnessTool for QueryRefiner {
    fn name(&self) -> &'static str {
        "QueryRefiner"
    }

    async fn execute(&self, ctx: &mut PipelineContext, _state: &AppState) -> Result<(), HarnessError> {
        if ctx.original_messages.is_empty() {
            return Err(HarnessError::ExecutionFailed("No messages to refine".into()));
        }

        let mut refined = ctx.original_messages.clone();

        if let Some(last) = refined.last_mut() {
            if last.role == "user" {
                let original = last.content_str().to_string();
                last.content = Some(format!("{}{}", original, REFINER_SUFFIX));
            }
        }

        ctx.refined_messages = refined;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_state() -> AppState {
        use crate::config::Config;

        let config = Config {
            port: 9999,
            workers: vec![],
            judge: crate::config::ModelConfig {
                name: "j".into(),
                endpoint: "http://localhost:11434".into(),
                model_id: "m".into(),
                api_key: None,
            },
            executor: crate::config::ModelConfig {
                name: "e".into(),
                endpoint: "http://localhost:11434".into(),
                model_id: "m".into(),
                api_key: None,
            },
            workspaces: std::collections::HashMap::new(),
            error_keywords: vec![],
            fusion: Default::default(),
        };
        AppState::new(config)
    }

    use crate::harness::PipelineContext;
    use crate::types::Message;

    #[tokio::test]
    async fn test_query_refiner_appends_suffix() {
        let state = mock_state();
        let mut ctx = PipelineContext::new(
            "test".into(),
            vec![Message::user("Fix this bug")],
            vec!["m1".into()],
            "judge".into(),
            false,
        );

        QueryRefiner.execute(&mut ctx, &state).await.unwrap();

        let last = ctx.refined_messages.last().unwrap();
        assert!(last.content_str().contains("Fix this bug"));
        assert!(last.content_str().contains("multiple perspectives"));
    }

    #[tokio::test]
    async fn test_query_refiner_empty_messages_error() {
        let state = mock_state();
        let mut ctx = PipelineContext::new(
            "test".into(),
            vec![],
            vec![],
            "judge".into(),
            false,
        );

        let result = QueryRefiner.execute(&mut ctx, &state).await;
        assert!(result.is_err());
    }
}

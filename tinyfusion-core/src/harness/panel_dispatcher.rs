use async_trait::async_trait;
use std::time::Instant;

use super::{HarnessError, HarnessTool, PipelineContext};
use crate::chat::AppState;
use crate::types::{FusionGuard, PanelResponse};

/// Module B: PanelDispatcher — concurrently fans out requests to panel models,
/// with per-model timeout and graceful degradation (at least 1 success required).
pub struct PanelDispatcher;

#[async_trait]
impl HarnessTool for PanelDispatcher {
    fn name(&self) -> &'static str {
        "PanelDispatcher"
    }

    async fn execute(&self, ctx: &mut PipelineContext, state: &AppState) -> Result<(), HarnessError> {
        let timeout_secs = state.config.fusion.timeout_seconds;
        let messages = &ctx.refined_messages;

        let mut handles = Vec::new();

        for model_name in &ctx.panel_models {
            let model_entry = state.config.fusion.get_model(model_name).cloned();
            let model_name = model_name.clone();
            let client = state.client.clone();
            let msgs = messages.clone();
            let timeout = timeout_secs;

            let handle = tokio::spawn(async move {
                let start = Instant::now();
                let result = call_panel_model(&client, &model_name, model_entry.as_ref(), &msgs, timeout).await;
                let latency_ms = start.elapsed().as_millis() as u64;

                PanelResponse {
                    model_name,
                    content: result,
                    latency_ms,
                    token_count: None,
                }
            });
            handles.push(handle);
        }

        let mut responses = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(resp) => responses.push(resp),
                Err(e) => responses.push(PanelResponse {
                    model_name: "unknown".into(),
                    content: Err(format!("Task join error: {}", e)),
                    latency_ms: 0,
                    token_count: None,
                }),
            }
        }

        let success_count = responses.iter().filter(|r| r.content.is_ok()).count();
        if success_count == 0 {
            tracing::error!("[PanelDispatcher] All {} panel models failed", responses.len());
            return Err(HarnessError::AllPanelsFailed);
        }

        tracing::info!(
            "[PanelDispatcher] {}/{} panels succeeded",
            success_count,
            responses.len()
        );

        ctx.panel_responses = responses;
        Ok(())
    }
}

/// Call a single panel model endpoint.
async fn call_panel_model(
    client: &reqwest::Client,
    model_name: &str,
    model_entry: Option<&crate::config::ModelEntry>,
    messages: &[crate::types::Message],
    timeout_secs: u64,
) -> Result<String, String> {
    let entry = model_entry.ok_or_else(|| {
        format!("Model '{}' not found in fusion.models registry", model_name)
    })?;

    let url = crate::proxy::build_chat_url_with_path(&entry.endpoint, entry.chat_path.as_deref());

    tracing::info!(
        "[PanelModel] Calling {} → {} (model_id: {})",
        model_name,
        url,
        entry.model_id
    );

    let body = serde_json::json!({
        "model": entry.model_id,
        "messages": messages,
        "stream": false,
    });

    let mut req = client.post(&url).json(&body);

    req = req.header(FusionGuard::HEADER_NAME, "1");

    req = crate::proxy::add_bearer_auth(req, entry.api_key.as_deref());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        req.send(),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            tracing::info!(
                "[PanelModel] {} responded with status {}",
                model_name,
                response.status()
            );
            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Panel {} returned {}: {}", model_name, status, text));
            }
            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse panel response: {}", e))?;

            extract_content_from_response(&json)
                .ok_or_else(|| format!("Panel {} returned no content", model_name))
        }
        Ok(Err(e)) => {
            tracing::error!("[PanelModel] {} request error: {}", model_name, e);
            Err(format!("Panel {} request failed: {}", model_name, e))
        }
        Err(_) => {
            tracing::error!("[PanelModel] {} timed out after {}s", model_name, timeout_secs);
            Err(format!("Panel {} timed out after {}s", model_name, timeout_secs))
        }
    }
}

/// Extract the assistant's content from a standard OpenAI chat completion response.
pub(crate) fn extract_content_from_response(json: &serde_json::Value) -> Option<String> {
    json["choices"]
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_content_from_response() {
        let json = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello world"
                }
            }]
        });
        assert_eq!(extract_content_from_response(&json), Some("Hello world".into()));
    }

    #[test]
    fn test_extract_content_from_empty_response() {
        let json = serde_json::json!({ "choices": [] });
        assert_eq!(extract_content_from_response(&json), None);
    }

    #[test]
    fn test_extract_content_from_malformed_response() {
        let json = serde_json::json!({ "error": "something" });
        assert_eq!(extract_content_from_response(&json), None);
    }
}

use async_trait::async_trait;

use super::{HarnessError, HarnessTool, PipelineContext};
use crate::chat::AppState;
use crate::types::{FusionGuard, StructuredAnalysis};

/// Module C: JudgeSynthesizer — sends panel responses to the judge model
/// with a strict JSON Schema system prompt, producing a StructuredAnalysis.
pub struct JudgeSynthesizer;

const JUDGE_SYSTEM_PROMPT: &str = r#"You are a technical judge synthesizing analyses from multiple AI models.

You MUST output a single JSON object with EXACTLY these fields:
{
  "consensus": ["<points all models agreed on>"],
  "contradictions": [{"topic": "<topic>", "stances": [{"model": "<name>", "stance": "<position>"}]}],
  "partial_coverage": [{"models": ["<model names>"], "point": "<insight only some covered>"}],
  "unique_insights": [{"model": "<name>", "insight": "<valuable unique point>"}],
  "blind_spots": ["<important aspects no model addressed>"]
}

Rules:
- Output ONLY valid JSON. No markdown, no explanation, no preamble.
- Every field is required. Use empty arrays [] if nothing applies.
- Be concise but precise in your analysis.
- Identify genuine disagreements, not just different phrasing of the same idea."#;

#[async_trait]
impl HarnessTool for JudgeSynthesizer {
    fn name(&self) -> &'static str {
        "JudgeSynthesizer"
    }

    async fn execute(&self, ctx: &mut PipelineContext, state: &AppState) -> Result<(), HarnessError> {
        let successful_responses: Vec<_> = ctx
            .panel_responses
            .iter()
            .filter(|r| r.content.is_ok())
            .collect();

        if successful_responses.is_empty() {
            return Err(HarnessError::ExecutionFailed(
                "No successful panel responses to synthesize".into(),
            ));
        }

        let user_prompt = build_judge_user_prompt(&ctx.original_messages, &successful_responses);

        let model_entry = state
            .config
            .fusion
            .get_model(&ctx.judge_model)
            .cloned()
            .ok_or_else(|| {
                HarnessError::ExecutionFailed(format!(
                    "Judge model '{}' not found in fusion.models registry",
                    ctx.judge_model
                ))
            })?;

        let url = format!(
            "{}/chat/completions",
            model_entry.endpoint.trim_end_matches('/')
        );

        let body = serde_json::json!({
            "model": model_entry.model_id,
            "messages": [
                {"role": "system", "content": JUDGE_SYSTEM_PROMPT},
                {"role": "user", "content": user_prompt}
            ],
            "stream": false,
            "response_format": {"type": "json_object"},
            "temperature": 0.1,
        });

        let mut req = state.client.post(&url).json(&body);
        req = req.header(FusionGuard::HEADER_NAME, "1");
        if let Some(ref key) = model_entry.api_key {
            if !key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
        }

        let timeout = state.config.fusion.timeout_seconds;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            req.send(),
        )
        .await;

        let response = match result {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                return Err(HarnessError::ExecutionFailed(format!(
                    "Judge request failed: {}",
                    e
                )));
            }
            Err(_) => return Err(HarnessError::Timeout),
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(HarnessError::ExecutionFailed(format!(
                "Judge returned {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| HarnessError::ExecutionFailed(format!("Failed to parse judge response: {}", e)))?;

        let content = json["choices"]
            .as_array()
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                HarnessError::ExecutionFailed("Judge response missing content".into())
            })?;

        let analysis = parse_judge_json(content)?;
        ctx.structured_analysis = Some(analysis);
        Ok(())
    }
}

/// Parse the judge's JSON output into a StructuredAnalysis.
/// Falls back to a minimal structure if JSON parsing fails.
fn parse_judge_json(content: &str) -> Result<StructuredAnalysis, HarnessError> {
    serde_json::from_str::<StructuredAnalysis>(content).map_err(|e| {
        tracing::warn!(
            "Judge output was not valid StructuredAnalysis JSON: {}. Raw: {}",
            e,
            &content[..content.len().min(500)]
        );
        HarnessError::SchemaValidationFailed(format!(
            "Judge JSON parse error: {}",
            e
        ))
    })
}

/// Build the user prompt for the judge model, including all panel responses.
fn build_judge_user_prompt(
    original_messages: &[crate::types::Message],
    panel_responses: &[&crate::types::PanelResponse],
) -> String {
    let mut prompt = String::new();

    prompt.push_str("## Original User Query\n");
    for msg in original_messages {
        if msg.role == "user" {
            prompt.push_str(msg.content_str());
            prompt.push('\n');
        }
    }

    prompt.push_str("\n## Panel Model Responses\n");
    for (i, resp) in panel_responses.iter().enumerate() {
        prompt.push_str(&format!(
            "\n### Model {} ({})\n",
            i + 1,
            resp.model_name
        ));
        match &resp.content {
            Ok(content) => prompt.push_str(content),
            Err(err) => prompt.push_str(&format!("[Error: {}]", err)),
        }
        prompt.push('\n');
    }

    prompt.push_str("\nAnalyze these responses and output the structured JSON.");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PanelResponse;

    #[test]
    fn test_parse_judge_json_valid() {
        let json = r#"{
            "consensus": ["point1"],
            "contradictions": [],
            "partial_coverage": [],
            "unique_insights": [],
            "blind_spots": ["gap1"]
        }"#;
        let analysis = parse_judge_json(json).unwrap();
        assert_eq!(analysis.consensus.len(), 1);
        assert_eq!(analysis.blind_spots.len(), 1);
    }

    #[test]
    fn test_parse_judge_json_invalid() {
        let result = parse_judge_json("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_build_judge_user_prompt() {
        let messages = vec![crate::types::Message::user("How do I fix this?")];
        let responses = vec![PanelResponse {
            model_name: "gpt-4o".into(),
            content: Ok("Check the null pointer".into()),
            latency_ms: 200,
            token_count: None,
        }];
        let refs: Vec<&PanelResponse> = responses.iter().collect();
        let prompt = build_judge_user_prompt(&messages, &refs);
        assert!(prompt.contains("How do I fix this?"));
        assert!(prompt.contains("gpt-4o"));
        assert!(prompt.contains("Check the null pointer"));
    }
}

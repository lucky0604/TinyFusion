/// Multi-of-Agents (MoA) module.
///
/// Handles spawning concurrent worker model calls, collecting responses,
/// and constructing/forwarding to the Judge model for synthesis.

use crate::sniffer::Message;

/// Result from a single worker model call.
#[derive(Debug, Clone)]
pub struct WorkerResponse {
    pub endpoint: String,
    pub content: Result<String, String>, // Ok(response) or Err(error)
}

/// Call worker endpoints concurrently and collect responses.
///
/// Returns a vector of worker responses. Failed calls return Err in content.
pub async fn call_workers(
    workers: &[WorkerConfig],
    messages: &[Message],
    timeout_secs: u64,
) -> Vec<WorkerResponse> {
    let mut handles = Vec::new();

    for worker in workers {
        let worker = worker.clone();
        let messages = messages.to_vec();
        let handle = tokio::spawn(async move {
            call_worker(&worker, &messages, timeout_secs).await
        });
        handles.push(handle);
    }

    let mut responses = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(resp) => responses.push(resp),
            Err(e) => responses.push(WorkerResponse {
                endpoint: String::new(),
                content: Err(format!("Task failed: {}", e)),
            }),
        }
    }

    responses
}

/// Call a single worker endpoint.
async fn call_worker(
    config: &WorkerConfig,
    messages: &[Message],
    timeout_secs: u64,
) -> WorkerResponse {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": config.model_id,
        "messages": messages.iter().map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        }).collect::<Vec<_>>(),
    });

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        client
            .post(&config.endpoint)
            .json(&body)
            .send(),
    )
    .await;

    match result {
        Ok(Ok(response)) => match response.text().await {
            Ok(text) => WorkerResponse {
                endpoint: config.endpoint.clone(),
                content: Ok(text),
            },
            Err(e) => WorkerResponse {
                endpoint: config.endpoint.clone(),
                content: Err(format!("Failed to read response: {}", e)),
            },
        },
        Ok(Err(e)) => WorkerResponse {
            endpoint: config.endpoint.clone(),
            content: Err(format!("Request failed: {}", e)),
        },
        Err(_) => WorkerResponse {
            endpoint: config.endpoint.clone(),
            content: Err(format!("Worker timed out after {}s", timeout_secs)),
        },
    }
}

/// Construct the Judge system prompt requesting XML-formatted diagnostic output.
pub fn build_judge_prompt(
    original_prompt: &str,
    worker_responses: &[WorkerResponse],
) -> String {
    let mut prompt = String::new();

    prompt.push_str("You are a technical judge reviewing analyses from multiple AI models.\n");
    prompt.push_str("Review their responses and produce a structured diagnostic in XML format.\n");
    prompt.push_str("Use the following tags:\n");
    prompt.push_str("- <consensus>: Points all workers agreed on\n");
    prompt.push_str("- <contradictions>: Where workers disagree\n");
    prompt.push_str("- <coverage_gaps>: Important aspects no worker covered\n");
    prompt.push_str("- <unique_insights>: Valuable points from individual workers\n");
    prompt.push_str("- <final_plan>: The recommended action plan (required)\n\n");

    prompt.push_str("Original request:\n");
    prompt.push_str(original_prompt);
    prompt.push_str("\n\nWorker responses:\n");

    for (i, resp) in worker_responses.iter().enumerate() {
        prompt.push_str(&format!("\n--- Worker {} ({}) ---\n", i + 1, resp.endpoint));
        match &resp.content {
            Ok(content) => prompt.push_str(content),
            Err(err) => prompt.push_str(&format!("[Error: {}]", err)),
        }
    }

    prompt.push_str("\n\nProvide your analysis using the XML tags specified above.");
    prompt
}

/// Parse XML tags from Judge response text.
#[derive(Debug)]
pub struct JudgeAnalysis {
    pub consensus: String,
    pub contradictions: String,
    pub coverage_gaps: String,
    pub unique_insights: String,
    pub final_plan: String,
    pub raw: String, // Full raw response if XML parsing is partial
}

pub fn parse_judge_xml(text: &str) -> JudgeAnalysis {
    let consensus = extract_tag(text, "consensus");
    let contradictions = extract_tag(text, "contradictions");
    let coverage_gaps = extract_tag(text, "coverage_gaps");
    let unique_insights = extract_tag(text, "unique_insights");
    let final_plan = extract_tag(text, "final_plan");

    let effective_final_plan = if final_plan.is_empty()
        && consensus.is_empty()
        && contradictions.is_empty()
        && coverage_gaps.is_empty()
        && unique_insights.is_empty()
    {
        text.trim().to_string()
    } else {
        final_plan
    };

    JudgeAnalysis {
        consensus,
        contradictions,
        coverage_gaps,
        unique_insights,
        final_plan: effective_final_plan,
        raw: text.to_string(),
    }
}

fn extract_tag(text: &str, tag: &str) -> String {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let (Some(start), Some(end)) = (text.find(&open), text.find(&close)) {
        let content_start = start + open.len();
        if content_start <= end {
            return text[content_start..end].trim().to_string();
        }
    }
    String::new()
}

/// Worker configuration.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub endpoint: String,
    pub model_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_judge_prompt_construction() {
        let workers = vec![
            WorkerResponse {
                endpoint: "http://localhost:11434".into(),
                content: Ok("Analysis A".into()),
            },
        ];
        let prompt = build_judge_prompt("Fix the bug", &workers);
        assert!(prompt.contains("<consensus>"));
        assert!(prompt.contains("<contradictions>"));
        assert!(prompt.contains("<final_plan>"));
        assert!(prompt.contains("Fix the bug"));
        assert!(prompt.contains("Analysis A"));
    }

    #[test]
    fn test_xml_tag_parsing() {
        let xml = r#"
            <consensus>All agree on root cause</consensus>
            <contradictions>Worker 2 suggests different fix</contradictions>
            <coverage_gaps>None identified</coverage_gaps>
            <unique_insights>Worker 1 found edge case</unique_insights>
            <final_plan>Step 1: Fix X. Step 2: Test Y.</final_plan>
        "#;
        let analysis = parse_judge_xml(xml);
        assert_eq!(analysis.consensus, "All agree on root cause");
        assert_eq!(analysis.contradictions, "Worker 2 suggests different fix");
        assert_eq!(analysis.final_plan, "Step 1: Fix X. Step 2: Test Y.");
    }

    #[test]
    fn test_xml_parse_fallback() {
        let text = "No XML tags here, just plain text";
        let analysis = parse_judge_xml(text);
        assert_eq!(analysis.consensus, "");
        assert_eq!(analysis.raw, text);
    }

    #[test]
    fn test_xml_parse_partial() {
        let xml = "<consensus>Agreed</consensus><final_plan>Do it</final_plan>";
        let analysis = parse_judge_xml(xml);
        assert_eq!(analysis.consensus, "Agreed");
        assert_eq!(analysis.final_plan, "Do it");
        assert_eq!(analysis.contradictions, "");
    }

    #[test]
    fn test_xml_fallback_uses_raw_text_as_final_plan() {
        let text = "The fix is to update auth.ts:47 and add a null check.";
        let analysis = parse_judge_xml(text);
        assert_eq!(analysis.consensus, "");
        assert_eq!(analysis.contradictions, "");
        assert_eq!(analysis.coverage_gaps, "");
        assert_eq!(analysis.unique_insights, "");
        assert_eq!(analysis.final_plan, text);
        assert_eq!(analysis.raw, text);
    }

    #[test]
    fn test_xml_fallback_no_false_positive_when_tags_present() {
        let xml = "<consensus>Fix auth.ts</consensus><final_plan>Do it</final_plan>";
        let analysis = parse_judge_xml(xml);
        assert_eq!(analysis.consensus, "Fix auth.ts");
        assert_eq!(analysis.final_plan, "Do it");
        assert_ne!(analysis.final_plan, analysis.raw);
    }
}

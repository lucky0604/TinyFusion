use crate::config::{ClassifierConfig, ModelTier, RoutingConfig};

/// Classify request complexity based on the latest user message.
///
/// Heuristics (applied to the last user message):
/// - Message length (chars)
/// - Error keyword count (from config.error_keywords)
/// - File mention count (paths like `src/foo.rs`, `./bar.ts`)
pub fn classify_complexity(
    messages: &[crate::types::Message],
    error_keywords: &[String],
    routing: &RoutingConfig,
) -> ModelTier {
    let last_user = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content_str())
        .unwrap_or("");

    if last_user.is_empty() {
        return ModelTier::Simple;
    }

    let lower = last_user.to_lowercase();
    let len = last_user.len();

    let error_count = error_keywords
        .iter()
        .filter(|kw| lower.contains(&kw.to_lowercase()))
        .count();

    let file_mentions = count_file_mentions(last_user);

    // Complex: long message, OR multiple error keywords, OR many file mentions
    if len >= routing.complex_threshold
        || error_count >= 2
        || file_mentions >= routing.file_mention_threshold
    {
        return ModelTier::Complex;
    }

    // Medium: moderate length, OR has error keywords, OR has file mentions
    if len >= routing.medium_threshold || error_count >= 1 || file_mentions >= 1 {
        return ModelTier::Medium;
    }

    ModelTier::Simple
}

/// Count likely file path mentions in text.
/// Matches patterns like: `src/foo.rs`, `./bar.ts`, `path/to/file.py`
fn count_file_mentions(text: &str) -> usize {
    let extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java",
        ".c", ".cpp", ".h", ".hpp", ".rb", ".swift", ".kt", ".toml",
        ".json", ".yaml", ".yml", ".md", ".css", ".html", ".vue",
        ".svelte",
    ];

    text.split_whitespace()
        .filter(|word| {
            let w = word.trim_matches(|c: char| c == '`' || c == '\'' || c == '"' || c == ',');
            w.contains('/') && extensions.iter().any(|ext| w.ends_with(ext))
        })
        .count()
}

const CLASSIFIER_SYSTEM_PROMPT: &str = "\
You are a request complexity classifier. Analyze the user's message and reply with exactly one word: SIMPLE or COMPLEX.

SIMPLE: greetings, short factual questions, translations, simple explanations, casual conversation, single-concept lookups.
COMPLEX: debugging with error logs, multi-file code changes, architecture design, refactoring tasks, build/test failures, tasks requiring analysis of multiple components.

Reply with ONLY the word SIMPLE or COMPLEX. Nothing else.";

/// Build the classifier prompt from the user's messages.
/// Returns (system_message, user_summary) for the classifier call.
pub fn build_classifier_prompt(messages: &[crate::types::Message]) -> (String, String) {
    let last_user = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content_str().to_string())
        .unwrap_or_default();

    // Truncate to avoid wasting tokens on very long messages
    let truncated = if last_user.len() > 500 {
        let end = last_user
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= 500)
            .last()
            .unwrap_or(0);
        format!("{}...", &last_user[..end])
    } else {
        last_user
    };

    (CLASSIFIER_SYSTEM_PROMPT.to_string(), truncated)
}

/// Parse the classifier model's response into a boolean (true = Simple).
pub fn parse_classifier_response(response: &str) -> Option<bool> {
    let upper = response.trim().to_uppercase();
    if upper.contains("SIMPLE") && !upper.contains("COMPLEX") {
        Some(true)
    } else if upper.contains("COMPLEX") {
        Some(false)
    } else {
        None
    }
}

/// Call an AI model to classify request complexity.
/// Returns `Some(true)` for Simple, `Some(false)` for Complex, `None` on failure.
pub async fn classify_with_ai(
    client: &reqwest::Client,
    messages: &[crate::types::Message],
    classifier_config: &ClassifierConfig,
) -> Option<bool> {
    let (system_prompt, user_msg) = build_classifier_prompt(messages);

    let url = crate::proxy::build_chat_url_with_path(
        &classifier_config.endpoint,
        classifier_config.chat_path.as_deref(),
    );

    let body = serde_json::json!({
        "model": classifier_config.model_id,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_msg },
        ],
        "max_tokens": 10,
        "temperature": 0.0,
        "stream": false,
    });

    let mut req = client.post(&url).json(&body);
    if let Some(key) = &classifier_config.api_key {
        if !key.is_empty() {
            req = req.bearer_auth(key);
        }
    }

    let timeout = std::time::Duration::from_secs(classifier_config.timeout_secs);

    tracing::info!(
        "[Classifier] Calling {} → {} (timeout: {}s)",
        classifier_config.model_id,
        url,
        classifier_config.timeout_secs,
    );

    let resp = match tokio::time::timeout(timeout, req.send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            tracing::warn!("[Classifier] Request failed: {}", e);
            return None;
        }
        Err(_) => {
            tracing::warn!("[Classifier] Timed out after {}s", classifier_config.timeout_secs);
            return None;
        }
    };

    if !resp.status().is_success() {
        tracing::warn!("[Classifier] HTTP {}", resp.status());
        return None;
    }

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("[Classifier] Failed to parse response: {}", e);
            return None;
        }
    };

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");

    let result = parse_classifier_response(content);
    tracing::info!(
        "[Classifier] Response: {:?} → {:?}",
        content.trim(),
        result.map(|s| if s { "SIMPLE" } else { "COMPLEX" }),
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RoutingConfig;
    use crate::types::Message;

    fn default_routing() -> RoutingConfig {
        RoutingConfig::default()
    }

    fn default_keywords() -> Vec<String> {
        vec![
            "compile error".into(),
            "test failed".into(),
            "build failed".into(),
            "panic".into(),
        ]
    }

    #[test]
    fn test_empty_messages_is_simple() {
        let result = classify_complexity(&[], &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Simple);
    }

    #[test]
    fn test_short_message_is_simple() {
        let msgs = vec![Message::user("Hello, how are you?")];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Simple);
    }

    #[test]
    fn test_error_keyword_triggers_medium() {
        let msgs = vec![Message::user("I got a compile error in my code")];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Medium);
    }

    #[test]
    fn test_multiple_error_keywords_triggers_complex() {
        let msgs = vec![Message::user("compile error and test failed in module")];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Complex);
    }

    #[test]
    fn test_file_mentions_trigger_medium() {
        let msgs = vec![Message::user("Fix the bug in src/main.rs")];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Medium);
    }

    #[test]
    fn test_many_file_mentions_trigger_complex() {
        let msgs = vec![Message::user(
            "Refactor src/main.rs src/lib.rs src/config.rs to use the new pattern",
        )];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Complex);
    }

    #[test]
    fn test_long_message_triggers_complex() {
        let long_msg = "a".repeat(800);
        let msgs = vec![Message::user(&long_msg)];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Complex);
    }

    #[test]
    fn test_medium_length_message() {
        let medium_msg = "a".repeat(200);
        let msgs = vec![Message::user(&medium_msg)];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Medium);
    }

    #[test]
    fn test_uses_last_user_message() {
        let msgs = vec![
            Message::user("This is a very long initial message ".repeat(30).trim()),
            Message::assistant("Sure, I can help."),
            Message::user("ok"),
        ];
        let result = classify_complexity(&msgs, &default_keywords(), &default_routing());
        assert_eq!(result, ModelTier::Simple);
    }

    #[test]
    fn test_custom_routing_thresholds() {
        let routing = RoutingConfig {
            medium_threshold: 10,
            complex_threshold: 50,
            file_mention_threshold: 2,
        };
        let msgs = vec![Message::user("Hello world!")]; // 12 chars
        let result = classify_complexity(&msgs, &[], &routing);
        assert_eq!(result, ModelTier::Medium);
    }

    #[test]
    fn test_count_file_mentions() {
        assert_eq!(count_file_mentions("fix src/main.rs"), 1);
        assert_eq!(count_file_mentions("edit src/main.rs and src/lib.rs"), 2);
        assert_eq!(count_file_mentions("hello world"), 0);
        assert_eq!(count_file_mentions("`src/config.toml`"), 1);
    }

    // --- AI classifier tests ---

    #[test]
    fn test_parse_classifier_response_simple() {
        assert_eq!(parse_classifier_response("SIMPLE"), Some(true));
        assert_eq!(parse_classifier_response("  simple  "), Some(true));
        assert_eq!(parse_classifier_response("Simple\n"), Some(true));
    }

    #[test]
    fn test_parse_classifier_response_complex() {
        assert_eq!(parse_classifier_response("COMPLEX"), Some(false));
        assert_eq!(parse_classifier_response("  complex  "), Some(false));
        assert_eq!(parse_classifier_response("Complex\n"), Some(false));
    }

    #[test]
    fn test_parse_classifier_response_garbage() {
        assert_eq!(parse_classifier_response(""), None);
        assert_eq!(parse_classifier_response("I think it's moderate"), None);
        assert_eq!(parse_classifier_response("42"), None);
    }

    #[test]
    fn test_parse_classifier_response_both_keywords_prefers_complex() {
        // COMPLEX takes precedence (safer fallback to MoA)
        assert_eq!(
            parse_classifier_response("SIMPLE but also COMPLEX"),
            Some(false),
        );
    }

    #[test]
    fn test_build_classifier_prompt_extracts_last_user() {
        let msgs = vec![
            Message::user("first question"),
            Message::assistant("response"),
            Message::user("second question"),
        ];
        let (system, user) = build_classifier_prompt(&msgs);
        assert!(system.contains("SIMPLE or COMPLEX"));
        assert_eq!(user, "second question");
    }

    #[test]
    fn test_build_classifier_prompt_truncates_long_messages() {
        let long_msg = "x".repeat(1000);
        let msgs = vec![Message::user(&long_msg)];
        let (_, user) = build_classifier_prompt(&msgs);
        assert!(user.len() < 600);
        assert!(user.ends_with("..."));
    }

    #[test]
    fn test_build_classifier_prompt_empty_messages() {
        let msgs: Vec<Message> = vec![];
        let (_, user) = build_classifier_prompt(&msgs);
        assert!(user.is_empty());
    }
}

use crate::config::{ModelTier, RoutingConfig};

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
}

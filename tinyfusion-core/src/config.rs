use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

/// Model endpoint configuration used for workers, judge, and executor (legacy).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub endpoint: String,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// Complexity tier for smart routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    Simple,
    Medium,
    Complex,
}

/// A model entry in the Unified Model Registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub provider: String,
    pub endpoint: String,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Complexity tier this model is assigned to (for smart routing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
    /// Whether this model runs locally (zero cost for budget tracking).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_local: Option<bool>,
    /// Custom chat completions path (e.g. "/api/paas/v4/chat/completions" for Zhipu).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_path: Option<String>,
}

/// Smart routing thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Minimum message length (chars) to consider Medium complexity.
    #[serde(default = "default_medium_threshold")]
    pub medium_threshold: usize,
    /// Minimum message length (chars) to consider Complex.
    #[serde(default = "default_complex_threshold")]
    pub complex_threshold: usize,
    /// File mention count threshold for Complex.
    #[serde(default = "default_file_mention_threshold")]
    pub file_mention_threshold: usize,
}

fn default_medium_threshold() -> usize { 200 }
fn default_complex_threshold() -> usize { 800 }
fn default_file_mention_threshold() -> usize { 3 }

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            medium_threshold: default_medium_threshold(),
            complex_threshold: default_complex_threshold(),
            file_mention_threshold: default_file_mention_threshold(),
        }
    }
}

/// Token budget limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Daily cloud token limit (0 = unlimited).
    #[serde(default)]
    pub daily_limit: u64,
    /// Monthly cloud token limit (0 = unlimited).
    #[serde(default)]
    pub monthly_limit: u64,
    /// How often to persist budget state to disk (seconds).
    #[serde(default = "default_persist_interval")]
    pub persist_interval_secs: u64,
}

fn default_persist_interval() -> u64 { 60 }

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            daily_limit: 0,
            monthly_limit: 0,
            persist_interval_secs: default_persist_interval(),
        }
    }
}

/// AI complexity classifier configuration.
/// When set, `model: "tinyfusion"` requests are pre-classified before routing:
/// Simple requests skip MoA and go directly to `simple_target`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifierConfig {
    /// API endpoint base URL (e.g. "https://api.example.com/v1").
    pub endpoint: String,
    /// API key for the classifier model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Model ID to send in the request (e.g. "qwen3-8b").
    pub model_id: String,
    /// Custom chat completions path (e.g. "/api/paas/v4/chat/completions").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_path: Option<String>,
    /// Timeout in seconds for the classifier call. Default: 5.
    #[serde(default = "default_classifier_timeout")]
    pub timeout_secs: u64,
    /// Model name (key in fusion.models) to forward Simple requests to.
    pub simple_target: String,
}

fn default_classifier_timeout() -> u64 { 5 }

/// Fusion deliberation pipeline configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionConfig {
    #[serde(default = "default_outer_model")]
    pub default_outer_model: String,
    #[serde(default = "default_judge_model")]
    pub default_judge_model: String,
    #[serde(default = "default_fusion_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub presets: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub models: HashMap<String, ModelEntry>,
    /// Planned: enable multi-turn debate between panel models (not yet implemented).
    #[serde(default)]
    pub enable_debate: bool,
    /// Planned: enable fact-checking pass after judge synthesis (not yet implemented).
    #[serde(default)]
    pub enable_fact_check: bool,
    #[serde(default)]
    pub enable_self_healing: bool,
    /// Smart routing configuration.
    #[serde(default)]
    pub routing: Option<RoutingConfig>,
    /// Token budget configuration.
    #[serde(default)]
    pub budget: Option<BudgetConfig>,
    /// AI complexity classifier: pre-classify requests before MoA.
    #[serde(default)]
    pub classifier: Option<ClassifierConfig>,
}

fn default_outer_model() -> String {
    "default".into()
}
fn default_judge_model() -> String {
    "default".into()
}
fn default_fusion_timeout() -> u64 {
    30
}

impl Default for FusionConfig {
    fn default() -> Self {
        Self {
            default_outer_model: default_outer_model(),
            default_judge_model: default_judge_model(),
            timeout_seconds: default_fusion_timeout(),
            presets: HashMap::new(),
            models: HashMap::new(),
            enable_debate: false,
            enable_fact_check: false,
            enable_self_healing: false,
            routing: None,
            budget: None,
            classifier: None,
        }
    }
}

impl FusionConfig {
    /// Look up a model entry by its registry name.
    pub fn get_model(&self, name: &str) -> Option<&ModelEntry> {
        self.models.get(name)
    }

    /// Resolve panel models from a preset name, or return the raw list.
    pub fn resolve_panel_models(&self, models: &[String]) -> Vec<String> {
        if models.len() == 1 {
            if let Some(preset) = self.presets.get(&models[0]) {
                return preset.clone();
            }
        }
        models.to_vec()
    }
}

/// Workspace entry: maps a workspace name to its path, verify command, and retry/timeout settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub path: String,
    pub verify_command: String,
    #[serde(default = "default_verify_timeout")]
    pub verify_timeout_seconds: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_verify_timeout() -> u64 {
    45
}
fn default_max_retries() -> u32 {
    3
}

/// Top-level TinyFusion configuration loaded from ~/.tinyfusion/config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    /// Legacy: worker model configurations. Kept for backward compatibility.
    #[serde(default)]
    pub workers: Vec<ModelConfig>,
    /// Legacy: judge model. Kept for backward compatibility.
    #[serde(default = "default_legacy_model")]
    pub judge: ModelConfig,
    /// Legacy: executor model. Kept for backward compatibility.
    #[serde(default = "default_legacy_model")]
    pub executor: ModelConfig,
    #[serde(default)]
    pub workspaces: HashMap<String, WorkspaceConfig>,
    #[serde(default = "default_error_keywords")]
    pub error_keywords: Vec<String>,
    /// New Fusion deliberation pipeline configuration.
    #[serde(default)]
    pub fusion: FusionConfig,
}

fn default_legacy_model() -> ModelConfig {
    ModelConfig {
        name: "default".into(),
        endpoint: "http://localhost:11434".into(),
        model_id: "llama3".into(),
        api_key: None,
    }
}

fn default_error_keywords() -> Vec<String> {
    vec![
        "stack trace".into(),
        "compile error".into(),
        "test failed".into(),
        "stacktrace".into(),
        "compilation error".into(),
        "tests failed".into(),
        "build failed".into(),
        "assertion error".into(),
        "panic".into(),
    ]
}

impl Config {
    /// Resolve the config file path: ~/.tinyfusion/config.json
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".tinyfusion").join("config.json")
    }

    /// Build a sensible default configuration.
    fn default_config() -> Self {
        Config {
            port: 9999,
            workers: vec![ModelConfig {
                name: "ollama-default".to_string(),
                endpoint: "http://localhost:11434".to_string(),
                model_id: "llama3".to_string(),
                api_key: None,
            }],
            judge: ModelConfig {
                name: "judge".to_string(),
                endpoint: "http://localhost:11434".to_string(),
                model_id: "llama3".to_string(),
                api_key: None,
            },
            executor: ModelConfig {
                name: "executor".to_string(),
                endpoint: "http://localhost:11434".to_string(),
                model_id: "llama3".to_string(),
                api_key: None,
            },
            workspaces: HashMap::new(),
            error_keywords: default_error_keywords(),
            fusion: FusionConfig::default(),
        }
    }

    /// Load configuration from the given path.
    ///
    /// If the file does not exist, creates the parent directory and writes a
    /// default config, then returns it.
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.exists() {
            // Create ~/.tinyfusion/ directory if missing
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }

            let default = Self::default_config();
            let json = serde_json::to_string_pretty(&default)?;
            fs::write(path, &json)?;
            info!("Created default config at {}", path.display());

            return Ok(default);
        }

        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;

        // Count distinct model endpoints for the log
        let model_count = config.model_count();
        info!(
            "Loaded config from {} ({} models, port {})",
            path.display(),
            model_count,
            config.port
        );

        // v1→v2 migration warnings
        config.warn_missing_v2_fields();

        Ok(config)
    }

    /// Convenience: load from the default path.
    pub fn load_default() -> Result<Self, Box<dyn std::error::Error>> {
        Self::load(&Self::default_path())
    }

    /// Count total configured model entries (workers + judge + executor).
    pub fn model_count(&self) -> usize {
        self.workers.len() + 1 /* judge */ + 1 /* executor */
    }

    /// Check for missing v2 configuration fields and log helpful warnings.
    fn warn_missing_v2_fields(&self) {
        let mut missing = Vec::new();

        if self.fusion.routing.is_none() {
            missing.push("fusion.routing");
        }
        if self.fusion.budget.is_none() {
            missing.push("fusion.budget");
        }

        let models_without_tier = self
            .fusion
            .models
            .iter()
            .filter(|(_, e)| e.tier.is_none())
            .count();
        if models_without_tier > 0 {
            tracing::warn!(
                "[Config v2] {} fusion model(s) have no 'tier' field — smart routing will use Medium as default",
                models_without_tier
            );
        }

        if !missing.is_empty() {
            tracing::warn!(
                "[Config v2] Missing optional sections: {}. \
                 Smart routing and budget tracking are disabled. \
                 Add to config.json to enable:\n\
                 \"fusion\": {{\n  \
                   \"routing\": {{ \"medium_threshold\": 200, \"complex_threshold\": 800, \"file_mention_threshold\": 3 }},\n  \
                   \"budget\": {{ \"daily_limit\": 500000, \"monthly_limit\": 5000000, \"persist_interval_secs\": 60 }}\n\
                 }}",
                missing.join(", ")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: write a JSON string to a temp file and return (path, dir_for_cleanup).
    /// The caller should remove the temp dir after the test.
    fn write_config_temp(content: &str) -> (PathBuf, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "tinyfusion_cfg_test_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos(),
            unique
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, content).unwrap();
        (path, dir)
    }

    #[test]
    fn test_load_valid_config() {
        let json = serde_json::json!({
            "port": 8888,
            "workers": [
                {
                    "name": "worker-1",
                    "endpoint": "http://localhost:11434",
                    "model_id": "llama3"
                },
                {
                    "name": "worker-2",
                    "endpoint": "https://api.openai.com/v1",
                    "model_id": "gpt-4",
                    "api_key": "sk-test"
                }
            ],
            "judge": {
                "name": "judge",
                "endpoint": "http://localhost:11434",
                "model_id": "llama3"
            },
            "executor": {
                "name": "executor",
                "endpoint": "http://localhost:11434",
                "model_id": "llama3"
            },
            "workspaces": {
                "my-project": {
                    "path": "/home/user/project",
                    "verify_command": "cargo test",
                    "verify_timeout_seconds": 60,
                    "max_retries": 5
                }
            }
        });

        let (path, dir) = write_config_temp(&json.to_string());
        let config = Config::load(&path).unwrap();

        assert_eq!(config.port, 8888);
        assert_eq!(config.workers.len(), 2);
        assert_eq!(config.workers[0].name, "worker-1");
        assert_eq!(config.workers[1].api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.judge.model_id, "llama3");
        assert_eq!(config.executor.endpoint, "http://localhost:11434");
        assert!(config.workspaces.contains_key("my-project"));
        assert_eq!(
            config.workspaces["my-project"].verify_command,
            "cargo test"
        );
        assert_eq!(config.workspaces["my-project"].verify_timeout_seconds, 60);
        assert_eq!(config.workspaces["my-project"].max_retries, 5);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_creates_default_if_missing() {
        let dir = std::env::temp_dir().join(format!("tinyfusion_test_{}", std::process::id()));
        let path = dir.join("config.json");

        // Ensure path does not exist
        let _ = fs::remove_dir_all(&dir);

        let config = Config::load(&path).unwrap();

        assert!(path.exists());
        assert_eq!(config.port, 9999);
        assert_eq!(config.workers.len(), 1);
        assert_eq!(config.workers[0].name, "ollama-default");
        assert!(config.workspaces.is_empty());

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_model_count() {
        let json = serde_json::json!({
            "port": 9999,
            "workers": [
                {"name": "w1", "endpoint": "http://localhost:11434", "model_id": "m1"},
                {"name": "w2", "endpoint": "http://localhost:11434", "model_id": "m2"},
                {"name": "w3", "endpoint": "http://localhost:11434", "model_id": "m3"}
            ],
            "judge": {"name": "judge", "endpoint": "http://localhost:11434", "model_id": "llama3"},
            "executor": {"name": "executor", "endpoint": "http://localhost:11434", "model_id": "llama3"},
            "workspaces": {}
        });

        let (path, dir) = write_config_temp(&json.to_string());
        let config = Config::load(&path).unwrap();

        // 3 workers + 1 judge + 1 executor = 5
        assert_eq!(config.model_count(), 5);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_empty_workers_config() {
        let json = serde_json::json!({
            "port": 7777,
            "workers": [],
            "judge": {"name": "judge", "endpoint": "http://localhost:11434", "model_id": "llama3"},
            "executor": {"name": "executor", "endpoint": "http://localhost:11434", "model_id": "llama3"},
            "workspaces": {}
        });

        let (path, dir) = write_config_temp(&json.to_string());
        let config = Config::load(&path).unwrap();

        assert_eq!(config.port, 7777);
        assert!(config.workers.is_empty());
        assert_eq!(config.model_count(), 2); // judge + executor only

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let original = Config::default_config();
        let json = serde_json::to_string_pretty(&original).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(original.port, loaded.port);
        assert_eq!(original.workers.len(), loaded.workers.len());
        assert_eq!(original.workers[0].name, loaded.workers[0].name);
        assert_eq!(original.judge.model_id, loaded.judge.model_id);
        assert_eq!(original.executor.endpoint, loaded.executor.endpoint);
    }

    #[test]
    fn test_default_path_structure() {
        let path = Config::default_path();
        // Should end with .tinyfusion/config.json
        assert!(path.ends_with("config.json"));
        assert!(path.to_string_lossy().contains(".tinyfusion"));
    }

    #[test]
    fn test_error_keywords_default() {
        let default = Config::default_config();
        assert!(default.error_keywords.contains(&"stack trace".into()));
        assert!(default.error_keywords.contains(&"build failed".into()));
        assert!(default.error_keywords.contains(&"panic".into()));
        assert_eq!(default.error_keywords.len(), 9);
    }

    #[test]
    fn test_error_keywords_custom() {
        let json = serde_json::json!({
            "port": 9999,
            "workers": [],
            "judge": {"name": "j", "endpoint": "http://localhost:11434", "model_id": "m"},
            "executor": {"name": "e", "endpoint": "http://localhost:11434", "model_id": "m"},
            "workspaces": {},
            "error_keywords": ["segfault", "null pointer"]
        });

        let (path, dir) = write_config_temp(&json.to_string());
        let config = Config::load(&path).unwrap();
        assert_eq!(config.error_keywords, vec!["segfault", "null pointer"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_fusion_config_get_model_found() {
        let mut config = Config::default_config();
        config.fusion.models.insert("m1".into(), crate::config::ModelEntry {
            provider: "test".into(),
            endpoint: "http://localhost:1".into(),
            model_id: "m1-id".into(),
            api_key: Some("k1".into()),
            tier: None,
            is_local: None,
            chat_path: None,
        });
        let entry = config.fusion.get_model("m1");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().model_id, "m1-id");
    }

    #[test]
    fn test_fusion_config_get_model_not_found() {
        let config = Config::default_config();
        assert!(config.fusion.get_model("nonexistent").is_none());
    }

    #[test]
    fn test_resolve_panel_models_from_preset() {
        let mut config = Config::default_config();
        config.fusion.presets.insert("fast".into(), vec!["a".into(), "b".into()]);
        let resolved = config.fusion.resolve_panel_models(&["fast".into()]);
        assert_eq!(resolved, vec!["a", "b"]);
    }

    #[test]
    fn test_resolve_panel_models_fallback() {
        let config = Config::default_config();
        let resolved = config.fusion.resolve_panel_models(&["m1".into(), "m2".into()]);
        assert_eq!(resolved, vec!["m1", "m2"]);
    }

    #[test]
    fn test_config_default_includes_fusion() {
        let config = Config::default_config();
        assert_eq!(config.fusion.timeout_seconds, 30);
        assert!(config.fusion.models.is_empty());
    }

    #[test]
    fn test_v1_config_backward_compat() {
        // A v1 config without any fusion.routing/budget/tier fields should load fine
        let json = serde_json::json!({
            "port": 9999,
            "workers": [],
            "judge": {"name": "j", "endpoint": "http://localhost:11434", "model_id": "m"},
            "executor": {"name": "e", "endpoint": "http://localhost:11434", "model_id": "m"},
            "workspaces": {},
            "fusion": {
                "models": {
                    "model-a": {
                        "provider": "openai",
                        "endpoint": "http://localhost:1234/v1",
                        "model_id": "gpt-4o",
                        "api_key": "sk-test"
                    }
                }
            }
        });

        let (path, dir) = write_config_temp(&json.to_string());
        let config = Config::load(&path).unwrap();
        assert!(config.fusion.routing.is_none());
        assert!(config.fusion.budget.is_none());
        let entry = config.fusion.get_model("model-a").unwrap();
        assert!(entry.tier.is_none());
        assert!(entry.is_local.is_none());
        assert!(entry.chat_path.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_v2_config_with_routing_and_budget() {
        let json = serde_json::json!({
            "port": 9999,
            "workers": [],
            "judge": {"name": "j", "endpoint": "http://localhost:11434", "model_id": "m"},
            "executor": {"name": "e", "endpoint": "http://localhost:11434", "model_id": "m"},
            "workspaces": {},
            "fusion": {
                "routing": {
                    "medium_threshold": 300,
                    "complex_threshold": 1000,
                    "file_mention_threshold": 5
                },
                "budget": {
                    "daily_limit": 500000,
                    "monthly_limit": 5000000,
                    "persist_interval_secs": 120
                },
                "models": {
                    "qwythos": {
                        "provider": "local",
                        "endpoint": "http://gpu:8080/v1",
                        "model_id": "qwythos-9b",
                        "tier": "simple",
                        "is_local": true
                    },
                    "deepseek": {
                        "provider": "deepseek",
                        "endpoint": "https://api.deepseek.com/v1",
                        "model_id": "deepseek-chat",
                        "api_key": "sk-ds",
                        "tier": "medium"
                    }
                }
            }
        });

        let (path, dir) = write_config_temp(&json.to_string());
        let config = Config::load(&path).unwrap();

        let routing = config.fusion.routing.as_ref().unwrap();
        assert_eq!(routing.medium_threshold, 300);
        assert_eq!(routing.complex_threshold, 1000);
        assert_eq!(routing.file_mention_threshold, 5);

        let budget = config.fusion.budget.as_ref().unwrap();
        assert_eq!(budget.daily_limit, 500000);
        assert_eq!(budget.monthly_limit, 5000000);

        let qwythos = config.fusion.get_model("qwythos").unwrap();
        assert_eq!(qwythos.tier, Some(ModelTier::Simple));
        assert_eq!(qwythos.is_local, Some(true));

        let deepseek = config.fusion.get_model("deepseek").unwrap();
        assert_eq!(deepseek.tier, Some(ModelTier::Medium));
        assert!(deepseek.is_local.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_model_entry_chat_path() {
        let json = serde_json::json!({
            "provider": "zhipu",
            "endpoint": "https://open.bigmodel.cn",
            "model_id": "glm-5.2",
            "api_key": "key",
            "chat_path": "/api/paas/v4/chat/completions"
        });

        let entry: ModelEntry = serde_json::from_value(json).unwrap();
        assert_eq!(
            entry.chat_path.as_deref(),
            Some("/api/paas/v4/chat/completions")
        );
    }

    #[test]
    fn test_model_tier_serialization() {
        assert_eq!(
            serde_json::to_string(&ModelTier::Simple).unwrap(),
            "\"simple\""
        );
        assert_eq!(
            serde_json::to_string(&ModelTier::Complex).unwrap(),
            "\"complex\""
        );
        let tier: ModelTier = serde_json::from_str("\"medium\"").unwrap();
        assert_eq!(tier, ModelTier::Medium);
    }
}

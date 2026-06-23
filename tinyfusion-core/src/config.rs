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

/// A model entry in the Unified Model Registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub provider: String,
    pub endpoint: String,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

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
}

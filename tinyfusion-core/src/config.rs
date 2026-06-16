use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

/// Model endpoint configuration used for workers, judge, and executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub endpoint: String,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// Workspace entry: maps a workspace name to its path and verify command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub path: String,
    pub verify_command: String,
}

/// Top-level TinyFusion configuration loaded from ~/.tinyfusion/config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    pub workers: Vec<ModelConfig>,
    pub judge: ModelConfig,
    pub executor: ModelConfig,
    pub workspaces: HashMap<String, WorkspaceConfig>,
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
        let dir = std::env::temp_dir().join(format!(
            "tinyfusion_cfg_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
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
                    "verify_command": "cargo test"
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
}

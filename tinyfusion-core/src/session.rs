use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::Mutex;

/// States a session can be in during the MoA lifecycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionState {
    Diagnostic,
    Execution,
    Verify,
    Done,
}

/// A tracked session with its current state and metadata.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub state: SessionState,
    pub retry_count: u32,
    pub messages: Vec<crate::sniffer::Message>,
    pub created_at: std::time::SystemTime,
}

impl Session {
    /// Create a new session with the given identifier.
    pub fn new(id: String, messages: Vec<crate::sniffer::Message>) -> Self {
        Self {
            id,
            state: SessionState::Diagnostic,
            retry_count: 0,
            messages,
            created_at: std::time::SystemTime::now(),
        }
    }

    /// Generate a session ID from the hash of concatenated messages.
    pub fn id_from_messages(messages: &[crate::sniffer::Message]) -> String {
        let mut hasher = Sha256::new();
        for msg in messages {
            hasher.update(msg.role.as_bytes());
            hasher.update(msg.content.as_bytes());
        }
        let result = hasher.finalize();
        hex::encode(result)
    }
}

/// In-memory session store.
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Session>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub id: String,
    pub state: SessionState,
    pub retry_count: u32,
    pub messages: Vec<crate::sniffer::Message>,
    pub created_at_secs: u64,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn snapshot_path() -> std::path::PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home)
            .join(".tinyfusion")
            .join("sessions.json")
    }

    pub fn save_snapshot(&self) {
        let sessions = self.sessions.lock().unwrap();
        let snapshots: Vec<SessionSnapshot> = sessions
            .iter()
            .filter(|(_, s)| s.state != SessionState::Done)
            .map(|(_, s)| SessionSnapshot {
                id: s.id.clone(),
                state: s.state.clone(),
                retry_count: s.retry_count,
                messages: s.messages.clone(),
                created_at_secs: s.created_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            })
            .collect();

        if snapshots.is_empty() {
            return;
        }

        let path = Self::snapshot_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&snapshots) {
            let _ = std::fs::write(&path, &json);
        }
    }

    pub fn load_snapshot(&self) -> usize {
        let path = Self::snapshot_path();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return 0,
        };

        let snapshots: Vec<SessionSnapshot> = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let mut sessions = self.sessions.lock().unwrap();
        let mut loaded = 0;
        for snap in &snapshots {
            if !sessions.contains_key(&snap.id) {
                let session = Session {
                    id: snap.id.clone(),
                    state: snap.state.clone(),
                    retry_count: snap.retry_count,
                    messages: snap.messages.clone(),
                    created_at: std::time::UNIX_EPOCH
                        + std::time::Duration::from_secs(snap.created_at_secs),
                };
                sessions.insert(snap.id.clone(), session);
                loaded += 1;
            }
        }

        let _ = std::fs::remove_file(&path);
        loaded
    }

    /// Create or retrieve a session by identifier.
    /// Returns (session_id, is_collision) — collision is true when a session
    /// exists with this hash but different messages (SHA-256 collision detected).
    pub fn get_or_create(
        &self,
        identifier: String,
        messages: Vec<crate::sniffer::Message>,
    ) -> (String, bool) {
        let mut sessions = self.sessions.lock().unwrap();

        if let Some(existing) = sessions.get(&identifier) {
            let existing_hash = Session::id_from_messages(&existing.messages);
            let new_hash = Session::id_from_messages(&messages);
            if existing_hash != new_hash {
                return (identifier, true);
            }
            return (identifier, false);
        }

        let session = Session::new(identifier.clone(), messages);
        sessions.insert(identifier.clone(), session);
        (identifier, false)
    }

    /// Look up a session by identifier.
    pub fn lookup(&self, id: &str) -> Option<Session> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
    }

    /// Update session state.
    pub fn set_state(&self, id: &str, state: SessionState) {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(id) {
            session.state = state;
        }
    }

    /// Increment retry counter.
    pub fn increment_retry(&self, id: &str) {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(id) {
            session.retry_count += 1;
        }
    }

    /// Append messages to a session.
    pub fn append_messages(&self, id: &str, new_messages: Vec<crate::sniffer::Message>) {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(id) {
            session.messages.extend(new_messages);
        }
    }

    /// Transition a session between states. Returns the new state on success.
    pub fn transition(&self, id: &str, max_retries: u32) -> Option<SessionState> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(id)?;

        let new_state = match session.state {
            SessionState::Diagnostic => SessionState::Execution,
            SessionState::Execution => SessionState::Verify,
            SessionState::Verify => {
                if session.retry_count < max_retries {
                    session.retry_count += 1;
                    SessionState::Diagnostic
                } else {
                    SessionState::Done
                }
            }
            SessionState::Done => return None,
        };

        session.state = new_state.clone();
        Some(new_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sniffer::Message;

    fn test_messages() -> Vec<Message> {
        vec![
            Message {
                role: "system".into(),
                content: "You are helpful".into(),
            },
            Message {
                role: "user".into(),
                content: "Fix this bug".into(),
            },
        ]
    }

    #[test]
    fn test_session_creation() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);

        let (returned_id, collision) = manager.get_or_create(id.clone(), messages);
        assert_eq!(returned_id, id);
        assert!(!collision);
        let lookup = manager.lookup(&id);
        assert!(lookup.is_some());
        let session = lookup.unwrap();
        assert_eq!(session.state, SessionState::Diagnostic);
        assert_eq!(session.retry_count, 0);
    }

    #[test]
    fn test_session_state_transition() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);

        let _ = manager.get_or_create(id.clone(), messages);

        manager.set_state(&id, SessionState::Execution);
        let session = manager.lookup(&id).unwrap();
        assert_eq!(session.state, SessionState::Execution);
    }

    #[test]
    fn test_session_collision_detection() {
        let manager = SessionManager::new();
        let messages1 = test_messages();

        let mut messages2 = messages1.clone();
        messages2[0].content = "Different content to force different hash".into();

        let id1 = Session::id_from_messages(&messages1);
        let id2 = Session::id_from_messages(&messages2);
        assert_ne!(id1, id2, "Test requires different hashes");

        manager.get_or_create(id1.clone(), messages1);

        let (_, collision) = manager.get_or_create(id1.clone(), messages2);
        assert!(collision);
    }

    #[test]
    fn test_session_hash_is_deterministic() {
        let messages = test_messages();
        let hash1 = Session::id_from_messages(&messages);
        let hash2 = Session::id_from_messages(&messages);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_session_retry_increment() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);

        let _ = manager.get_or_create(id.clone(), messages);

        manager.increment_retry(&id);
        manager.increment_retry(&id);
        let session = manager.lookup(&id).unwrap();
        assert_eq!(session.retry_count, 2);
    }

    #[test]
    fn test_transition_diagnostic_to_execution() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);
        let _ = manager.get_or_create(id.clone(), messages);

        let new_state = manager.transition(&id, 3).unwrap();
        assert_eq!(new_state, SessionState::Execution);
        assert_eq!(manager.lookup(&id).unwrap().state, SessionState::Execution);
    }

    #[test]
    fn test_transition_execution_to_verify() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);
        let _ = manager.get_or_create(id.clone(), messages);

        manager.set_state(&id, SessionState::Execution);
        let new_state = manager.transition(&id, 3).unwrap();
        assert_eq!(new_state, SessionState::Verify);
    }

    #[test]
    fn test_transition_verify_to_done_on_success() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);
        let _ = manager.get_or_create(id.clone(), messages);

        manager.set_state(&id, SessionState::Verify);
        // retry_count starts at 0, max_retries is 0 → should go to Done
        let new_state = manager.transition(&id, 0).unwrap();
        assert_eq!(new_state, SessionState::Done);
    }

    #[test]
    fn test_transition_verify_to_diagnostic_on_retry() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);
        let _ = manager.get_or_create(id.clone(), messages);

        manager.set_state(&id, SessionState::Verify);
        // retry_count 0 < max 3 → should retry to Diagnostic
        let new_state = manager.transition(&id, 3).unwrap();
        assert_eq!(new_state, SessionState::Diagnostic);
        assert_eq!(manager.lookup(&id).unwrap().retry_count, 1);
    }

    #[test]
    fn test_transition_done_returns_none() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);
        let _ = manager.get_or_create(id.clone(), messages);

        manager.set_state(&id, SessionState::Done);
        let result = manager.transition(&id, 3);
        assert!(result.is_none(), "Done state should not transition");
    }

    #[test]
    fn test_transition_verify_to_done_when_max_retries_reached() {
        let manager = SessionManager::new();
        let messages = test_messages();
        let id = Session::id_from_messages(&messages);
        let _ = manager.get_or_create(id.clone(), messages);

        manager.set_state(&id, SessionState::Verify);
        manager.increment_retry(&id);
        manager.increment_retry(&id);
        manager.increment_retry(&id); // retry_count = 3, max = 3

        let new_state = manager.transition(&id, 3).unwrap();
        assert_eq!(new_state, SessionState::Done);
    }
}

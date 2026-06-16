use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::Mutex;

/// States a session can be in during the MoA lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Diagnostic,
    Execution,
    Verify,
    Retry,
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

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Create or retrieve a session by identifier.
    /// Returns the session ID that was created or looked up.
    pub fn get_or_create(
        &self,
        identifier: String,
        messages: Vec<crate::sniffer::Message>,
    ) -> String {
        let mut sessions = self.sessions.lock().unwrap();

        if !sessions.contains_key(&identifier) {
            let session = Session::new(identifier.clone(), messages);
            sessions.insert(identifier.clone(), session);
        }

        identifier
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

        let returned_id = manager.get_or_create(id.clone(), messages);
        assert_eq!(returned_id, id);
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

        manager.get_or_create(id.clone(), messages);

        manager.set_state(&id, SessionState::Execution);
        let session = manager.lookup(&id).unwrap();
        assert_eq!(session.state, SessionState::Execution);
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

        manager.get_or_create(id.clone(), messages);

        manager.increment_retry(&id);
        manager.increment_retry(&id);
        let session = manager.lookup(&id).unwrap();
        assert_eq!(session.retry_count, 2);
    }
}

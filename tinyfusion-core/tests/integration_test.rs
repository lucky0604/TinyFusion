use tinyfusion_core::session::{Session, SessionManager, SessionState};
use tinyfusion_core::sniffer::{Message, RequestState};
use tinyfusion_core::sniffer;

#[test]
fn test_full_state_machine_cycle() {
    let manager = SessionManager::new();
    let messages = vec![
        Message { role: "system".into(), content: "You are a coding assistant".into() },
        Message { role: "user".into(), content: "Fix the compile error".into() },
    ];
    let id = Session::id_from_messages(&messages);

    assert_eq!(sniffer::sniff_state(&messages), RequestState::Diagnostic);

    manager.get_or_create(id.clone(), messages, vec![]);
    let session = manager.lookup(&id).unwrap();
    assert_eq!(session.state, SessionState::Diagnostic);

    let new_state = manager.transition(&id, 3).unwrap();
    assert_eq!(new_state, SessionState::Execution);
    assert_eq!(manager.lookup(&id).unwrap().state, SessionState::Execution);

    let new_state = manager.transition(&id, 3).unwrap();
    assert_eq!(new_state, SessionState::Verify);
    assert_eq!(manager.lookup(&id).unwrap().state, SessionState::Verify);

    manager.set_state(&id, SessionState::Done);
    assert!(manager.transition(&id, 3).is_none());
}

#[test]
fn test_diagnostic_triggered_by_error_keywords() {
    let test_cases = vec![
        vec![Message { role: "user".into(), content: "I got a stack trace: ...".into() }],
        vec![Message { role: "user".into(), content: "compile error in src/main.rs:42".into() }],
        vec![Message { role: "user".into(), content: "build failed with exit code 1".into() }],
        vec![Message { role: "user".into(), content: "assertion error: expected true".into() }],
        vec![Message { role: "user".into(), content: "thread panicked at src/lib.rs:10".into() }],
    ];

    for messages in &test_cases {
        assert_eq!(
            sniffer::sniff_state(messages),
            RequestState::Diagnostic,
            "Should be Diagnostic for: {:?}",
            messages
        );
    }
}

#[test]
fn test_execution_triggered_by_final_plan() {
    let messages = vec![
        Message { role: "system".into(), content: "You are an assistant".into() },
        Message { role: "user".into(), content: "Fix the bug".into() },
        Message { role: "assistant".into(), content: "Here is the plan: </final_plan> Apply changes to auth.ts".into() },
    ];

    assert_eq!(sniffer::sniff_state(&messages), RequestState::Execution);
}

#[test]
fn test_session_retry_loop() {
    let manager = SessionManager::new();
    let messages = vec![
        Message { role: "user".into(), content: "fix the bug".into() },
    ];
    let id = Session::id_from_messages(&messages);
    manager.get_or_create(id.clone(), messages, vec![]);

    for _ in 0..3 {
        manager.set_state(&id, SessionState::Verify);
        let new_state = manager.transition(&id, 3).unwrap();
        assert_eq!(new_state, SessionState::Diagnostic);
    }

    manager.set_state(&id, SessionState::Verify);
    let new_state = manager.transition(&id, 3).unwrap();
    assert_eq!(new_state, SessionState::Done);
}

#[test]
fn test_session_retry_counter() {
    let manager = SessionManager::new();
    let messages = vec![
        Message { role: "user".into(), content: "test".into() },
    ];
    let id = Session::id_from_messages(&messages);
    manager.get_or_create(id.clone(), messages, vec![]);

    for i in 1..=5 {
        manager.increment_retry(&id);
        let session = manager.lookup(&id).unwrap();
        assert_eq!(session.retry_count, i);
    }
}

#[test]
fn test_concurrent_sessions_independent() {
    let manager = SessionManager::new();

    let msgs1 = vec![
        Message { role: "user".into(), content: "Fix auth bug".into() },
    ];
    let msgs2 = vec![
        Message { role: "user".into(), content: "Fix database bug".into() },
    ];

    let id1 = Session::id_from_messages(&msgs1);
    let id2 = Session::id_from_messages(&msgs2);
    assert_ne!(id1, id2);

    manager.get_or_create(id1.clone(), msgs1, vec![]);
    manager.get_or_create(id2.clone(), msgs2, vec![]);

    manager.increment_retry(&id1);
    manager.increment_retry(&id1);
    manager.increment_retry(&id2);

    assert_eq!(manager.lookup(&id1).unwrap().retry_count, 2);
    assert_eq!(manager.lookup(&id2).unwrap().retry_count, 1);
}

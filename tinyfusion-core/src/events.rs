use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use serde::Serialize;
use std::convert::Infallible;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
pub struct GatewayEvent {
    pub event_type: String,
    pub session_id: Option<String>,
    pub message: String,
    pub timestamp: u64,
}

impl GatewayEvent {
    pub fn new(event_type: &str, message: &str) -> Self {
        Self {
            event_type: event_type.to_string(),
            session_id: None,
            message: message.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }
}

pub struct EventBus {
    tx: broadcast::Sender<GatewayEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn emit(&self, event: GatewayEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<GatewayEvent> {
        self.tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

pub fn event_stream(
    rx: broadcast::Receiver<GatewayEvent>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        yield Ok(Event::default().data(json));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
}

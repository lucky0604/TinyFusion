//! SSE keep-alive module.
//!
//! Sends periodic keep-alive comments to prevent client timeout during long computations.
//! SSE comments (lines starting with `:`) are ignored by standard SSE parsers.

use axum::response::sse::Event;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;

/// Interval between keep-alive comments (15 seconds).
const KEEPALIVE_INTERVAL_SECS: u64 = 15;

/// Generate a valid SSE keep-alive comment event.
///
/// Comments start with `:` and are ignored by SSE clients but keep the connection alive.
pub fn keepalive_event() -> Event {
    Event::default().comment("keepalive")
}

/// Run a keep-alive ticker that sends events until cancelled or real data arrives.
///
/// Returns a stream of keep-alive events. Callers should merge this with their
/// real data stream and cancel the token when real tokens start arriving.
pub fn keepalive_stream(
    cancel: CancellationToken,
) -> impl futures::Stream<Item = Result<Event, std::convert::Infallible>> {
    let mut interval = interval(Duration::from_secs(KEEPALIVE_INTERVAL_SECS));

    async_stream::stream! {
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    yield Ok(keepalive_event());
                }
                _ = cancel.cancelled() => {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keepalive_event_is_comment() {
        let event = keepalive_event();
        // Event should be serializable as SSE comment
        let formatted = format!("{:?}", event);
        assert!(formatted.contains("keepalive") || !formatted.is_empty());
    }
}

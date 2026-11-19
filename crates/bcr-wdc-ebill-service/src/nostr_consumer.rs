use std::{fmt::Display, future::Future, time::Duration};

use axum::{Router, http::StatusCode, routing::get};
use tokio::{sync::watch, task::JoinSet, time::Instant};
use tracing::{info, warn};

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);

/// Readiness of the consumer task, not a guarantee of relay connectivity or delivery.
pub fn health_router(readiness: watch::Receiver<bool>) -> Router {
    Router::new().route(
        "/health",
        get(move || {
            let readiness = readiness.clone();
            async move {
                // A closed channel also makes a panicked or cancelled supervisor unhealthy.
                if readiness.has_changed().is_ok() && *readiness.borrow() {
                    (StatusCode::OK, "OK")
                } else {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Nostr consumer unavailable",
                    )
                }
            }
        }),
    )
}

pub async fn run<S, SF, C, CF, E>(
    mut start: S,
    mut cleanup: C,
    restart_interval: Duration,
    readiness: watch::Sender<bool>,
) where
    S: FnMut() -> SF,
    SF: Future<Output = Result<JoinSet<()>, E>>,
    E: Display,
    C: FnMut() -> CF,
    CF: Future<Output = ()>,
{
    let restart_interval = restart_interval.max(INITIAL_RETRY_DELAY);
    let mut retry_delay = INITIAL_RETRY_DELAY;
    readiness.send_replace(false);

    loop {
        let scheduled_restart = match start().await {
            Err(error) => {
                warn!("Failed to start Nostr consumer: {error}");
                false
            }
            Ok(mut tasks) => {
                let running_since = Instant::now();
                let scheduled_restart = if tasks.is_empty() {
                    warn!("Nostr consumer started without any tasks");
                    false
                } else {
                    readiness.send_replace(true);
                    tokio::select! {
                        _ = tokio::time::sleep(restart_interval) => {
                            info!("Restarting Nostr consumer to refresh subscriptions");
                            true
                        }
                        result = tasks.join_next() => {
                            warn!("Nostr consumer task stopped: {result:?}");
                            false
                        }
                    }
                };

                readiness.send_replace(false);
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}

                // Briefly starting successfully must not reset backoff for a crashing task.
                if running_since.elapsed() >= MAX_RETRY_DELAY {
                    retry_delay = INITIAL_RETRY_DELAY;
                }
                scheduled_restart
            }
        };

        // start() can subscribe to private events before failing to subscribe to public ones.
        // Clean up those partial subscriptions too, before starting a new attempt.
        cleanup().await;

        if scheduled_restart {
            retry_delay = INITIAL_RETRY_DELAY;
        } else {
            warn!("Retrying Nostr consumer in {}s", retry_delay.as_secs());
            tokio::time::sleep(retry_delay).await;
            retry_delay = retry_delay.saturating_mul(2).min(MAX_RETRY_DELAY);
        }
    }
}

#[cfg(test)]
mod tests;

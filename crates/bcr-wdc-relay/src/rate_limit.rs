use std::collections::VecDeque;
use time::Duration;

const MAX_IDLE: Duration = Duration::seconds(24 * 3600); // remove after 24h idle
pub const PRUNE_INTERVAL: Duration = Duration::seconds(10 * 60); // check every 10 minutes

#[derive(Debug)]
pub struct SlidingWindow {
    hits: VecDeque<time::OffsetDateTime>,
    window: Duration,
    limit: usize,
    last_seen: time::OffsetDateTime,
}

impl SlidingWindow {
    pub fn new(limit: usize, window: Duration) -> Self {
        Self {
            hits: VecDeque::with_capacity(limit),
            window,
            limit,
            last_seen: time::OffsetDateTime::now_utc(),
        }
    }

    pub fn allow(&mut self, now: time::OffsetDateTime) -> bool {
        // Remove expired hits
        while let Some(&ts) = self.hits.front() {
            if now - ts > self.window {
                self.hits.pop_front();
            } else {
                break;
            }
        }
        self.last_seen = now;

        if self.hits.len() < self.limit {
            self.hits.push_back(now);
            true
        } else {
            false
        }
    }

    pub fn retain(&self, now: time::OffsetDateTime) -> bool {
        now - self.last_seen <= MAX_IDLE
    }
}

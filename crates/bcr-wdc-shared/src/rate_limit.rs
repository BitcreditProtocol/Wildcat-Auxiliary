use bcr_common::core::NodeId;
use chrono::TimeDelta;
use email_address::EmailAddress;
use std::collections::{HashMap, VecDeque};

use crate::{TStamp, now};

/// How often do we allow the same email to be registered in the time frame
const EMAIL_LIMIT: usize = 20;
const EMAIL_WINDOW: TimeDelta = TimeDelta::days(1);

/// How often do we allow the same nodeid in the time frame
const NODE_ID_LIMIT: usize = 50;
const NODE_ID_WINDOW: TimeDelta = TimeDelta::minutes(10);

const MAX_IDLE: TimeDelta = TimeDelta::days(1); // remove after 1 day idle
pub const PRUNE_INTERVAL: TimeDelta = TimeDelta::minutes(10); // check every 10 minutes

#[derive(Debug)]
pub struct SlidingWindow {
    hits: VecDeque<TStamp>,
    window: TimeDelta,
    limit: usize,
    last_seen: TStamp,
}

impl SlidingWindow {
    pub fn new(limit: usize, window: TimeDelta) -> Self {
        Self {
            hits: VecDeque::with_capacity(limit),
            window,
            limit,
            last_seen: now(),
        }
    }

    pub fn allow(&mut self, now: TStamp) -> bool {
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

    pub fn retain(&self, now: TStamp) -> bool {
        now - self.last_seen <= MAX_IDLE
    }
}

#[derive(Debug)]
pub struct RateLimiter {
    by_email: HashMap<EmailAddress, SlidingWindow>,
    by_node_id_sender: HashMap<NodeId, SlidingWindow>,
    by_node_id_receiver: HashMap<NodeId, SlidingWindow>,
    last_prune: TStamp,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            by_email: HashMap::new(),
            by_node_id_sender: HashMap::new(),
            by_node_id_receiver: HashMap::new(),
            last_prune: now(),
        }
    }

    /// Check if the request is allowed
    /// Everything that's set has to be allowed
    /// The values are expected to be validated before getting in here
    pub fn check(
        &mut self,
        email: Option<&EmailAddress>,
        node_id_sender: Option<&NodeId>,
        node_id_receiver: Option<&NodeId>,
    ) -> bool {
        let now = now();
        self.prune_if_needed(now);

        let email_ok = if let Some(email) = email {
            self.by_email
                .entry(email.to_owned())
                .or_insert_with(|| SlidingWindow::new(EMAIL_LIMIT, EMAIL_WINDOW))
                .allow(now)
        } else {
            true // no email provided -> skip check
        };

        let node_id_sender_ok = if let Some(node_id) = node_id_sender {
            self.by_node_id_sender
                .entry(node_id.to_owned())
                .or_insert_with(|| SlidingWindow::new(NODE_ID_LIMIT, NODE_ID_WINDOW))
                .allow(now)
        } else {
            true // no sender node_id provided -> skip check
        };

        let node_id_receiver_ok = if let Some(node_id) = node_id_receiver {
            self.by_node_id_receiver
                .entry(node_id.to_owned())
                .or_insert_with(|| SlidingWindow::new(NODE_ID_LIMIT, NODE_ID_WINDOW))
                .allow(now)
        } else {
            true // no receiver node_id provided -> skip check
        };

        email_ok && node_id_sender_ok && node_id_receiver_ok
    }

    /// Every PRUNE_INTERVAL, remove outdated entries
    fn prune_if_needed(&mut self, now: TStamp) {
        if now - self.last_prune < PRUNE_INTERVAL {
            return;
        }

        self.last_prune = now;

        // only keep recent entries
        self.by_email.retain(|_, win| win.retain(now));
        self.by_node_id_sender.retain(|_, win| win.retain(now));
        self.by_node_id_receiver.retain(|_, win| win.retain(now));
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

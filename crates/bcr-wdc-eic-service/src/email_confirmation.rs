use bcr_common::core::NodeId;
use bcr_wdc_shared::{TStamp, now};
use chrono::TimeDelta;
use email_address::EmailAddress;

/// Maximum age of an email confirmation
const EMAIL_CONFIRMATION_EXPIRY_SECONDS: i64 = 60 * 60 * 24; // 1 day

/// Maximum number of wrong confirmation code entries, before it gets deleted to avoid brute forcing other peoples addresses
const MAX_WRONG_TRIES: usize = 5;

#[derive(Debug, Clone)]
pub struct EmailConfirmation {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub confirmation_code: String,
    pub wrong_entries: usize,
    pub created_at: TStamp,
}

impl EmailConfirmation {
    pub fn is_expired(&self) -> bool {
        now()
            > self
                .created_at
                .checked_add_signed(TimeDelta::seconds(EMAIL_CONFIRMATION_EXPIRY_SECONDS))
                .expect("valid addition")
    }

    pub fn has_too_many_retries(&self) -> bool {
        self.wrong_entries >= MAX_WRONG_TRIES
    }

    pub fn matches_confirmation_code(&self, code: &str) -> bool {
        self.confirmation_code == code
    }
}

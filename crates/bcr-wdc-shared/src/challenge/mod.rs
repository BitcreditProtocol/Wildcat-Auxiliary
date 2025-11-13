use anyhow::Result;
use bitcoin::base58;
use chrono::TimeDelta;
use std::fmt;

pub mod persistence;

/// Maximum age of a challenge - we expect requests to be made immediately after each other
const CHALLENGE_EXPIRY: TimeDelta = TimeDelta::minutes(2);

/// 32 random bytes, base58 encoded
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge(String);

impl Default for Challenge {
    fn default() -> Self {
        Self::new()
    }
}

impl Challenge {
    pub fn new() -> Self {
        let challenge = base58::encode(&rand::random::<[u8; 32]>());
        Self(challenge)
    }

    pub fn ttl(&self) -> TimeDelta {
        CHALLENGE_EXPIRY
    }

    pub fn decode(&self) -> Result<Vec<u8>> {
        let res = base58::decode(&self.0)?;
        Ok(res)
    }
}

impl fmt::Display for Challenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for Challenge {
    fn from(value: String) -> Self {
        Self(value)
    }
}

use anyhow::{Result, anyhow};
use bcr_common::core::NodeId;
use bitcoin::base58;
use chrono::TimeDelta;
use secp256k1::schnorr::Signature;
use std::fmt;
use tracing::warn;

use crate::{TStamp, now, signature};

pub mod persistence;

/// 32 random bytes, base58 encoded
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge(String);

impl Default for Challenge {
    fn default() -> Self {
        Self::new()
    }
}

impl Challenge {
    /// Maximum age of a challenge - we expect requests to be made immediately after each other
    const CHALLENGE_EXPIRY: TimeDelta = TimeDelta::minutes(2);

    pub fn new() -> Self {
        let challenge = base58::encode(&rand::random::<[u8; 32]>());
        Self(challenge)
    }

    pub fn ttl(&self) -> TimeDelta {
        Self::CHALLENGE_EXPIRY
    }

    pub fn decode(&self) -> Result<Vec<u8>> {
        let res = base58::decode(&self.0)?;
        Ok(res)
    }

    pub fn check(
        &self,
        node_id: &NodeId,
        signed_challenge: &Signature,
        created_at: TStamp,
    ) -> Result<bool> {
        // check if challenge timed out
        if now()
            > (created_at
                .checked_add_signed(self.ttl())
                .expect("safe to add seconds"))
        {
            return Err(anyhow!("Challenge Timed Out"));
        }

        let x_only = node_id.pub_key().x_only_public_key().0;

        // check if challenge is valid
        match signature::verify_signature(
            &self.decode().map_err(|_| anyhow!("Invalid Challenge"))?,
            signed_challenge,
            &x_only,
        ) {
            Ok(true) => Ok(true),
            Ok(false) => Err(anyhow!("Invalid Challenge")),
            Err(e) => {
                warn!("Couldn't check challenge: {e}");
                Err(anyhow!("Error Checking Challenge"))
            }
        }
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

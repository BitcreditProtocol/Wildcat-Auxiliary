use crate::{
    TStamp, deserialize_as_str, deserialize_tstamp_as_u64, serialize_as_str,
    serialize_tstamp_as_u64,
};
use bcr_common::core::NodeId;
use borsh::{BorshDeserialize, BorshSerialize};
use email_address::EmailAddress;
use secp256k1::schnorr::Signature;
use serde::{Deserialize, Serialize};

// TODO: move to bcr-common

#[derive(Debug, Deserialize)]
pub struct ChallengeRequest {
    /// The caller node id
    pub node_id: NodeId,
}

#[derive(Debug, Serialize)]
pub struct ChallengeResponse {
    /// A random challenge to be signed by the caller to verify their identity on the following request
    pub challenge: String,
    /// The time the challenge is valid in seconds
    pub ttl: u64,
}

#[derive(Debug, Deserialize)]
pub struct EmailRegisterRequest {
    /// The caller node id
    pub node_id: NodeId,
    /// The caller company node id (optional)
    pub company_node_id: Option<NodeId>,
    /// The caller email
    pub email: EmailAddress,
    /// The signed challenge by the caller
    pub signed_challenge: Signature,
}

#[derive(Debug, Serialize)]
pub struct EmailRegisterResponse {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub struct EmailConfirmRequest {
    /// A borsh-encoded EmailConfirmPayload
    pub payload: String,
    /// The signature over the payload
    pub signature: Signature,
}

#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct EmailConfirmPayload {
    /// The caller node id
    pub node_id: NodeId,
    /// The caller company node id (optional)
    pub company_node_id: Option<NodeId>,
    /// The caller confirmation code
    pub confirmation_code: String,
}

#[derive(Debug, Serialize)]
pub struct EmailConfirmResponse {
    /// A borsh-encoded MintSignature
    pub payload: String,
    /// The mint signature of the payload
    pub signature: Signature,
    /// The mint node id
    pub mint_node_id: NodeId,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetEmailPreferencesRequest {
    /// The email owner node id
    pub node_id: NodeId,
    /// The email owner company node id (optional)
    pub company_node_id: Option<NodeId>,
    /// The email owner email
    pub email: EmailAddress,
}

#[derive(Debug, Serialize)]
pub struct SetEmailPreferencesResponse {
    pub success: bool,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct MintSignature {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    #[borsh(
        serialize_with = "serialize_as_str",
        deserialize_with = "deserialize_as_str"
    )]
    pub email: EmailAddress,
    #[borsh(
        serialize_with = "serialize_tstamp_as_u64",
        deserialize_with = "deserialize_tstamp_as_u64"
    )]
    pub created_at: TStamp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationSendRequest {
    /// The payload for the notification, borsh-encoded NotificationSendPayload
    pub payload: String,
    /// The payload signed by the sender
    pub signature: Signature,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct NotificationSendPayload {
    /// The type of event, e.g. BillSigned
    pub kind: String,
    /// The domain ID, e.g. a bill id
    pub id: String,
    /// The receiver node id
    pub receiver_node_id: NodeId,
    /// The receiver company_node id
    pub receiver_company_node_id: Option<NodeId>,
    /// The sender node id
    pub sender_node_id: NodeId,
}

#[derive(Debug, Serialize)]
pub struct NotificationSendResponse {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub struct GetEmailPreferencesLinkRequest {
    /// The caller node id
    pub node_id: NodeId,
    /// The caller company node id (optional)
    pub company_node_id: Option<NodeId>,
    /// The signed challenge by the caller
    pub signed_challenge: Signature,
}

#[derive(Debug, Serialize)]
pub struct GetEmailPreferencesLinkResponse {
    /// The preferences link
    pub preferences_link: url::Url,
}

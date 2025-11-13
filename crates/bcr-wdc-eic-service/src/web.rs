use crate::{
    error::{Error, Result},
    service::Service,
};
use axum::{Json, extract::State};
use bcr_wdc_shared::{
    signature::verify_request,
    wire::{
        ChallengeRequest, ChallengeResponse, EmailConfirmPayload, EmailConfirmRequest,
        EmailConfirmResponse, EmailRegisterRequest, EmailRegisterResponse,
    },
};
use bitcoin::base58;
use std::sync::Arc;

pub async fn health() -> &'static str {
    "{ \"status\": \"OK\" }"
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl))]
pub async fn challenge(
    State(ctrl): State<Arc<Service>>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>> {
    let challenge = ctrl.create_challenge_for_node_id(&req.node_id).await?;
    Ok(Json(ChallengeResponse {
        challenge: challenge.to_string(),
        ttl: challenge.ttl().num_seconds() as u64,
    }))
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl))]
pub async fn email_register(
    State(ctrl): State<Arc<Service>>,
    Json(req): Json<EmailRegisterRequest>,
) -> Result<Json<EmailRegisterResponse>> {
    ctrl.register_email_for_node_id(
        &req.node_id,
        &req.company_node_id,
        &req.email,
        &req.signed_challenge,
    )
    .await?;
    Ok(Json(EmailRegisterResponse { success: true }))
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl))]
pub async fn email_confirm(
    State(ctrl): State<Arc<Service>>,
    Json(req): Json<EmailConfirmRequest>,
) -> Result<Json<EmailConfirmResponse>> {
    let decoded = base58::decode(&req.payload)
        .map_err(|_| Error::SignedRequest("Invalid Payload (base58)".to_string()))?;

    let deserialized: EmailConfirmPayload = borsh::from_slice(&decoded)
        .map_err(|_| Error::SignedRequest("Invalid Payload (borsh)".into()))?;

    let Ok(true) = verify_request(
        &decoded,
        &req.signature,
        &deserialized.node_id.pub_key().x_only_public_key().0,
    ) else {
        return Err(Error::SignedRequest("Invalid Signature".into()));
    };

    let (mint_signature_payload, mint_signature, mint_node_id) = ctrl
        .confirm_email_for_node_id(
            &deserialized.node_id,
            &deserialized.company_node_id,
            &deserialized.confirmation_code,
        )
        .await?;

    let serialized = borsh::to_vec(&mint_signature_payload)
        .map_err(|_| Error::Signature("Invalid Payload (borsh)".into()))?;
    let payload = base58::encode(&serialized);

    Ok(Json(EmailConfirmResponse {
        payload,
        signature: mint_signature,
        mint_node_id,
    }))
}

use crate::error::Result;
use crate::service::Service;
use axum::{Json, extract::State};
use bcr_wdc_shared::wire::{
    ChallengeRequest, ChallengeResponse, SetEmailPreferencesRequest, SetEmailPreferencesResponse,
};
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
        ttl: challenge.ttl(),
    }))
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl))]
pub async fn set_email_preferences(
    State(ctrl): State<Arc<Service>>,
    Json(req): Json<SetEmailPreferencesRequest>,
) -> Result<Json<SetEmailPreferencesResponse>> {
    ctrl.set_email_notification_preferences(&req.node_id, &req.company_node_id, &req.email)
        .await?;
    Ok(Json(SetEmailPreferencesResponse { success: true }))
}

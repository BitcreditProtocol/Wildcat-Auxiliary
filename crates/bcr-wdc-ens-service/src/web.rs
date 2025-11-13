use crate::email_preferences::PreferencesFlags;
use crate::error::{Error, Result};
use crate::service::Service;
use crate::template::{ChangePreferencesPayload, build_html_error, build_template};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::{Json, extract::State};
use axum_extra::extract::Form;
use bcr_wdc_shared::rate_limit::RateLimiter;
use bcr_wdc_shared::{
    signature::verify_request,
    wire::{
        ChallengeRequest, ChallengeResponse, GetEmailPreferencesLinkRequest,
        GetEmailPreferencesLinkResponse, NotificationSendPayload, NotificationSendRequest,
        NotificationSendResponse, SetEmailPreferencesRequest, SetEmailPreferencesResponse,
    },
};
use bitcoin::base58;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, warn};
use uuid::Uuid;

pub async fn health() -> &'static str {
    "{ \"status\": \"OK\" }"
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl, req))]
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

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl, req))]
pub async fn set_email_preferences(
    State(ctrl): State<Arc<Service>>,
    Json(req): Json<SetEmailPreferencesRequest>,
) -> Result<Json<SetEmailPreferencesResponse>> {
    ctrl.set_email_notification_preferences(&req.node_id, &req.company_node_id, &req.email)
        .await?;
    Ok(Json(SetEmailPreferencesResponse { success: true }))
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl, req))]
pub async fn send_email(
    State(ctrl): State<Arc<Service>>,
    State(rl): State<Arc<Mutex<RateLimiter>>>,
    Json(req): Json<NotificationSendRequest>,
) -> Result<Json<NotificationSendResponse>> {
    let decoded = base58::decode(&req.payload)
        .map_err(|_| Error::SignedRequest("Invalid Payload (base58)".to_string()))?;

    let deserialized: NotificationSendPayload = borsh::from_slice(&decoded)
        .map_err(|_| Error::SignedRequest("Invalid Payload (borsh)".into()))?;

    let Ok(true) = verify_request(
        &decoded,
        &req.signature,
        &deserialized.sender_node_id.pub_key().x_only_public_key().0,
    ) else {
        return Err(Error::SignedRequest("Invalid Signature".into()));
    };

    let mut rate_limiter = rl.lock().await;
    let allowed = rate_limiter.check(
        None,
        Some(&deserialized.sender_node_id),
        Some(&deserialized.receiver_node_id),
    );
    drop(rate_limiter);
    if !allowed {
        warn!(
            "Rate limited req with sender node_id {}, receiver node id {}",
            &deserialized.sender_node_id, &deserialized.receiver_node_id
        );
        return Err(Error::RateLimit);
    }

    ctrl.send_email(
        &deserialized.receiver_node_id,
        &deserialized.receiver_company_node_id,
        &deserialized.kind,
        &deserialized.id,
    )
    .await?;

    Ok(Json(NotificationSendResponse { success: true }))
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl, req))]
pub async fn get_email_preferences_link(
    State(ctrl): State<Arc<Service>>,
    Json(req): Json<GetEmailPreferencesLinkRequest>,
) -> Result<Json<GetEmailPreferencesLinkResponse>> {
    ctrl.check_challenge(&req.node_id, &req.signed_challenge)
        .await?;
    let preferences_link = ctrl
        .get_preferences_link(&req.node_id, &req.company_node_id)
        .await?;
    Ok(Json(GetEmailPreferencesLinkResponse { preferences_link }))
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl))]
pub async fn preferences(
    State(ctrl): State<Arc<Service>>,
    Path(token): Path<Uuid>,
) -> impl IntoResponse {
    match ctrl.get_preferences(&token).await {
        Ok((tmpl, ctx)) => build_template(tmpl, ctx, StatusCode::OK).into_response(),
        Err(e) => {
            error!("Could not get preferences: {e}");
            build_html_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                &ctrl.get_logo_link(),
            )
            .into_response()
        }
    }
}

#[tracing::instrument(level = tracing::Level::DEBUG, skip(ctrl))]
pub async fn update_preferences(
    State(ctrl): State<Arc<Service>>,
    Form(payload): Form<ChangePreferencesPayload>,
) -> impl IntoResponse {
    let mut updated_preferences = PreferencesFlags::empty();
    // set all selected flags
    if let Some(ref flags) = payload.flags {
        for flag in flags {
            if let Some(parsed) = PreferencesFlags::from_bits(*flag) {
                updated_preferences |= parsed;
            }
        }
    }

    let enabled = match payload.enabled {
        Some(e) => e.as_str() == "on",
        None => false,
    };

    let preferences = payload.flags.map(|_| updated_preferences);

    match ctrl
        .update_preferences(&payload.pref_token, enabled, preferences)
        .await
    {
        Ok(()) => {
            Redirect::to(&format!("/email/preferences/{}", &payload.pref_token)).into_response()
        }
        Err(e) => {
            error!("Could not update preferences: {e}");
            build_html_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                &ctrl.get_logo_link(),
            )
            .into_response()
        }
    }
}

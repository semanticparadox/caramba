//! `POST /v1/activate` handler.
//!
//! Flow: look up the license key, enforce expiry, bind-or-check the instance id
//! (first activation binds; a single-seat key activated from a different
//! instance id is refused), sign the canonical message, and return the signed
//! [`ActivationResponse`]. The response BINDS the instance id, so it cannot be
//! replayed onto another instance.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use serde::Serialize;

use caramba_shared::license::{ActivationRequest, ActivationResponse, sign_activation};

use crate::state::ServerState;
use crate::store::KeyStore;

/// Error body returned to clients. Plain functional language, no internals.
#[derive(Debug, Serialize)]
pub struct ActivationError {
    pub error: String,
}

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<ActivationError>) {
    (
        status,
        Json(ActivationError {
            error: msg.to_string(),
        }),
    )
}

pub async fn activate(
    State(state): State<ServerState>,
    Json(req): Json<ActivationRequest>,
) -> Result<Json<ActivationResponse>, (StatusCode, Json<ActivationError>)> {
    let license_key = req.license_key.trim().to_string();
    let instance_id = req.instance_id.trim().to_string();

    if license_key.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "License key is required."));
    }
    if instance_id.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Instance id is required."));
    }

    // Serialize read-modify-write so concurrent first-activations cannot both
    // bind a different instance id to a single-seat key.
    let _guard = state.store_lock.lock().await;

    let mut store = KeyStore::load(&state.store_path).map_err(|e| {
        tracing::error!("key store load failed: {e:#}");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Activation is temporarily unavailable.",
        )
    })?;

    let entry = match store.keys.get(&license_key) {
        Some(e) => e.clone(),
        None => return Err(err(StatusCode::NOT_FOUND, "License key is not recognized.")),
    };

    // Expiry check.
    if entry.expires_at <= Utc::now() {
        return Err(err(StatusCode::FORBIDDEN, "License key has expired."));
    }

    // Instance binding. First activation binds; later activations from a
    // different instance id for a seat-capped key are refused.
    if !entry.is_bound(&instance_id) {
        if !entry.has_free_seat() {
            return Err(err(
                StatusCode::CONFLICT,
                "License key is already activated on another instance.",
            ));
        }
        // Bind this instance id and persist.
        if let Some(mut_entry) = store.keys.get_mut(&license_key) {
            mut_entry.bound_instance_ids.push(instance_id.clone());
        }
        store.save(&state.store_path).map_err(|e| {
            tracing::error!("key store save failed: {e:#}");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Activation is temporarily unavailable.",
            )
        })?;
        tracing::info!(
            "bound instance {} to key (tier {})",
            instance_id,
            entry.tier.as_str()
        );
    }

    // Sign the canonical, instance-bound message.
    let signature = sign_activation(
        &state.signing_key,
        &instance_id,
        &license_key,
        entry.tier,
        entry.expires_at,
        &entry.limits,
    );

    Ok(Json(ActivationResponse {
        tier: entry.tier,
        expires_at: entry.expires_at,
        limits: entry.limits,
        signature,
    }))
}

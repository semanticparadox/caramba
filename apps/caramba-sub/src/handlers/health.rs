use crate::AppState;
use axum::{extract::State, Json};
use serde_json::{json, Value};

pub async fn health_check(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "domain": state.config.domain,
        "region": state.config.region,
        "version": env!("CARGO_PKG_VERSION")
    }))
}

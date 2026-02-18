use axum::{
    extract::{State, Query},
    response::{IntoResponse, Json},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use crate::AppState;
use tracing::error;

#[derive(Deserialize)]
pub struct RecommendedQuery {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Serialize)]
pub struct RecommendedNode {
    pub id: i64,
    pub name: String,
    pub country_code: String,
    pub score: f64,
    pub distance_km: f64,
    pub load_pct: f64,
    pub latency_ms: f64,
}

#[derive(sqlx::FromRow)]
struct NodeRow {
    id: i64,
    name: String,
    country_code: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    last_latency: Option<f64>,
    last_cpu: Option<f64>,
    last_ram: Option<f64>,
}

#[derive(Deserialize)]
struct GeoIpResponse {
    lat: f64,
    lon: f64,
}

/// Get Recommended Nodes (AI Routing)
/// GET /api/v2/client/recommended
pub async fn get_recommended_nodes(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<RecommendedQuery>,
) -> impl IntoResponse {
    // 1. Determine User Location
    let (user_lat, user_lon) = if let (Some(lat), Some(lon)) = (query.lat, query.lon) {
        (lat, lon)
    } else {
        // Resolve from IP
        let remote_ip = headers
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(',').next())
            .unwrap_or("");

        if remote_ip.is_empty() || remote_ip == "127.0.0.1" {
            // Default to 0,0 if unknown
            (0.0, 0.0)
        } else {
            // Use ip-api.com (MVP) - ideally use local MaxMind DB
            let url = format!("http://ip-api.com/json/{}?fields=lat,lon", remote_ip);
            match reqwest::get(&url).await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<GeoIpResponse>().await {
                        (json.lat, json.lon)
                    } else {
                        (0.0, 0.0)
                    }
                },
                Err(_) => (0.0, 0.0)
            }
        }
    };

    // 2. Fetch Nodes
    let nodes: Vec<NodeRow> = match sqlx::query_as::<_, NodeRow>("SELECT id, name, country_code, latitude, longitude, last_latency, last_cpu, last_ram FROM nodes WHERE is_enabled = TRUE")
        .fetch_all(&state.pool)
        .await {
            Ok(n) => n,
            Err(e) => {
                error!("Failed to fetch nodes: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
            }
        };

    // 3. Score Nodes
    let mut scored_nodes: Vec<RecommendedNode> = nodes.into_iter().map(|n| {
        let node_lat = n.latitude.unwrap_or(0.0);
        let node_lon = n.longitude.unwrap_or(0.0);
        
        let dist = haversine(user_lat, user_lon, node_lat, node_lon);
        let lat = n.last_latency.unwrap_or(999.0); // Penalty if no latency
        let cpu = n.last_cpu.unwrap_or(0.0);
        let ram = n.last_ram.unwrap_or(0.0);
        let load = (cpu + ram) / 2.0;

        // Weights: Distance (1.0), Latency (0.5), Load (5.0)
        // Adjust these based on preference for speed vs proximity
        let score = (dist * 1.0) + (lat * 0.5) + (load * 5.0);
        
        RecommendedNode {
            id: n.id,
            name: n.name,
            country_code: n.country_code.unwrap_or("UNK".to_string()),
            score,
            distance_km: dist,
            load_pct: load,
            latency_ms: lat,
        }
    }).collect();

    // 4. Sort (Lowest score is best)
    scored_nodes.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));

    // 5. Return Top 3
    let top_nodes = scored_nodes.into_iter().take(3).collect::<Vec<_>>();

    Json(serde_json::json!({
        "user_location": { "lat": user_lat, "lon": user_lon },
        "nodes": top_nodes
    })).into_response()
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0; // Earth radius in km
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

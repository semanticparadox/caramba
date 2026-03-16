use crate::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Deserialize)]
pub struct RecommendedQuery {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub country: Option<String>,
    pub strategy: Option<String>,
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
    node_type: Option<String>,
    is_relay: Option<bool>,
}

#[derive(Clone, Copy)]
enum RoutingStrategy {
    Balanced,
    Fastest,
    Stable,
}

impl RoutingStrategy {
    fn from_query(raw: Option<&str>) -> Self {
        match raw
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("fastest") | Some("latency") => Self::Fastest,
            Some("stable") | Some("lowload") | Some("load") => Self::Stable,
            _ => Self::Balanced,
        }
    }

    fn weights(self) -> (f64, f64, f64) {
        match self {
            Self::Balanced => (1.0, 0.5, 5.0),
            Self::Fastest => (0.4, 1.5, 2.0),
            Self::Stable => (0.6, 0.6, 6.0),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Fastest => "fastest",
            Self::Stable => "stable",
        }
    }
}

fn normalize_country_filter(raw: Option<&str>) -> Option<String> {
    let normalized = raw
        .map(|value| value.trim().to_ascii_uppercase())
        .unwrap_or_default();

    if normalized.is_empty() || normalized == "ANY" || normalized == "AUTO" {
        return None;
    }

    if normalized.len() == 2 {
        Some(normalized)
    } else {
        None
    }
}

fn normalized_node_type(node_type: Option<&str>, is_relay: Option<bool>) -> &'static str {
    if node_type
        .map(|value| value.trim().eq_ignore_ascii_case("relay"))
        .unwrap_or(false)
        || is_relay.unwrap_or(false)
    {
        "relay"
    } else {
        "exit"
    }
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
    let strategy = RoutingStrategy::from_query(query.strategy.as_deref());
    let country_filter = normalize_country_filter(query.country.as_deref());

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
                }
                Err(_) => (0.0, 0.0),
            }
        }
    };

    // 2. Fetch Nodes (compat: fallback when node_type column is absent)
    let nodes: Vec<NodeRow> = match sqlx::query_as::<_, NodeRow>(
        "SELECT id, name, country_code, latitude, longitude, last_latency, last_cpu, last_ram, node_type, is_relay FROM nodes WHERE is_enabled = TRUE AND status = 'active'",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("node_type") && msg.contains("does not exist") {
                match sqlx::query_as::<_, NodeRow>(
                    "SELECT id, name, country_code, latitude, longitude, last_latency, last_cpu, last_ram, NULL::TEXT AS node_type, is_relay FROM nodes WHERE is_enabled = TRUE AND status = 'active'",
                )
                .fetch_all(&state.pool)
                .await
                {
                    Ok(rows) => rows,
                    Err(inner) => {
                        error!("Failed to fetch nodes (fallback): {}", inner);
                        return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
                    }
                }
            } else {
                error!("Failed to fetch nodes: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "DB Error").into_response();
            }
        }
    };

    let exit_nodes = nodes
        .into_iter()
        .filter(|node| normalized_node_type(node.node_type.as_deref(), node.is_relay) == "exit")
        .filter(|node| {
            country_filter.as_ref().map_or(true, |target| {
                node.country_code
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(target))
                    .unwrap_or(false)
            })
        })
        .collect::<Vec<_>>();

    // 3. Score Nodes
    let (w_distance, w_latency, w_load) = strategy.weights();
    let mut scored_nodes: Vec<RecommendedNode> = exit_nodes
        .into_iter()
        .map(|n| {
            let node_lat = n.latitude.unwrap_or(0.0);
            let node_lon = n.longitude.unwrap_or(0.0);

            let dist = haversine(user_lat, user_lon, node_lat, node_lon);
            let lat = n.last_latency.unwrap_or(999.0); // Penalty if no latency
            let cpu = n.last_cpu.unwrap_or(0.0);
            let ram = n.last_ram.unwrap_or(0.0);
            let load = (cpu + ram) / 2.0;

            let score = (dist * w_distance) + (lat * w_latency) + (load * w_load);

            RecommendedNode {
                id: n.id,
                name: n.name,
                country_code: n.country_code.unwrap_or("UNK".to_string()),
                score,
                distance_km: dist,
                load_pct: load,
                latency_ms: lat,
            }
        })
        .collect();

    // 4. Sort (Lowest score is best)
    scored_nodes.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 5. Return Top 3
    let top_nodes = scored_nodes.into_iter().take(3).collect::<Vec<_>>();

    Json(serde_json::json!({
        "user_location": { "lat": user_lat, "lon": user_lon },
        "strategy": strategy.as_str(),
        "country_filter": country_filter,
        "nodes": top_nodes
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::{RoutingStrategy, normalize_country_filter, normalized_node_type};

    #[test]
    fn country_filter_normalization_accepts_iso2_and_rejects_other_values() {
        assert_eq!(normalize_country_filter(Some("us")), Some("US".to_string()));
        assert_eq!(
            normalize_country_filter(Some(" De ")),
            Some("DE".to_string())
        );
        assert_eq!(normalize_country_filter(Some("ANY")), None);
        assert_eq!(normalize_country_filter(Some("auto")), None);
        assert_eq!(normalize_country_filter(Some("USA")), None);
        assert_eq!(normalize_country_filter(Some("1")), None);
        assert_eq!(normalize_country_filter(None), None);
    }

    #[test]
    fn strategy_normalization_maps_known_aliases() {
        assert!(matches!(
            RoutingStrategy::from_query(Some("balanced")),
            RoutingStrategy::Balanced
        ));
        assert!(matches!(
            RoutingStrategy::from_query(Some("latency")),
            RoutingStrategy::Fastest
        ));
        assert!(matches!(
            RoutingStrategy::from_query(Some("lowload")),
            RoutingStrategy::Stable
        ));
        assert!(matches!(
            RoutingStrategy::from_query(Some("unknown")),
            RoutingStrategy::Balanced
        ));
    }

    #[test]
    fn node_type_normalization_keeps_relay_nodes_out_of_exit_pool() {
        assert_eq!(normalized_node_type(Some("relay"), Some(false)), "relay");
        assert_eq!(normalized_node_type(Some("exit"), Some(false)), "exit");
        assert_eq!(normalized_node_type(None, Some(true)), "relay");
        assert_eq!(normalized_node_type(None, Some(false)), "exit");
    }
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0; // Earth radius in km
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

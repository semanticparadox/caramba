use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::time;
use tracing::{error, info, warn};

use crate::services::orchestration_service::OrchestrationService;
use crate::services::store_service::StoreService;

/// Represents a single connection from the Clash API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClashConnection {
    pub id: String,
    #[serde(rename = "metadata")]
    pub metadata: ConnectionMetadata,
    #[serde(default)]
    pub chains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionMetadata {
    #[serde(default)]
    pub network: String,
    #[serde(rename = "type")]
    pub conn_type: String,
    #[serde(rename = "sourceIP")]
    pub source_ip: String,
    #[serde(rename = "sourcePort")]
    pub source_port: String,
    #[serde(rename = "destinationIP")]
    pub destination_ip: String,
    #[serde(rename = "destinationPort")]
    pub destination_port: String,
    #[serde(default)]
    pub host: String,
    #[serde(rename = "inboundIP", default)]
    pub inbound_ip: String,
    #[serde(rename = "inboundPort", default)]
    pub inbound_port: String,
}

/// Clash API response for /connections
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClashConnectionsResponse {
    #[serde(default)]
    pub connections: Vec<ClashConnection>,
}

/// Connection monitoring and device limit enforcement service
pub struct ConnectionService {
    pool: SqlitePool,
    orchestration: Arc<OrchestrationService>,
    store: Arc<StoreService>,
}

impl ConnectionService {
    pub fn new(
        pool: SqlitePool,
        orchestration: Arc<OrchestrationService>,
        store: Arc<StoreService>,
    ) -> Self {
        Self {
            pool,
            orchestration,
            store,
        }
    }

    /// Start the background monitoring loop
    pub async fn start_monitoring(&self) {
        info!("Starting connection monitoring service for device limit enforcement...");
        let mut interval = time::interval(time::Duration::from_secs(300)); // 5 minutes

        loop {
            interval.tick().await;
            
            if let Err(e) = self.check_and_enforce_limits().await {
                error!("Error in connection monitoring cycle: {:#}", e);
            }

            // Cleanup old IP tracking records (>1 hour old)
            if let Err(e) = self.store.cleanup_old_ip_tracking().await {
                error!("Error cleaning up old IP tracking: {:#}", e);
            }
        }
    }

    /// Main enforcement logic: check all nodes and enforce device limits
    async fn check_and_enforce_limits(&self) -> Result<()> {
        info!("Running device limit enforcement cycle...");

        // Get all active nodes
        let nodes = self.orchestration.get_all_nodes().await?;
        
        if nodes.is_empty() {
            warn!("No active nodes found, skipping device limit check");
            return Ok(());
        }

        // Collect connections from all nodes
        let mut subscription_ips: HashMap<String, HashSet<String>> = HashMap::new();

        for node in nodes {
            if node.status != "active" {
                continue;
            }
            match self.fetch_node_connections(&node.ip).await {
                Ok(connections) => {
                    info!("Fetched {} connections from node {}", connections.len(), node.ip);
                    
                    // Group IPs by subscription UUID (extracted from inbound chains)
                    for conn in connections {
                        if let Some(uuid) = extract_uuid_from_connection(&conn) {
                            subscription_ips
                                .entry(uuid)
                                .or_insert_with(HashSet::new)
                                .insert(conn.metadata.source_ip.clone());
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to fetch connections from node {}: {:#}", node.ip, e);
                    continue;
                }
            }
        }

        info!("Collected IPs for {} subscriptions", subscription_ips.len());

        // Check each subscription's device limit
        for (uuid, ips) in subscription_ips {
            if let Err(e) = self.enforce_subscription_limit(&uuid, ips).await {
                error!("Failed to enforce limit for subscription {}: {:#}", uuid, e);
            }
        }

        Ok(())
    }

    /// Fetch active connections from a node's Clash API
    async fn fetch_node_connections(&self, node_host: &str) -> Result<Vec<ClashConnection>> {
        let url = format!("http://{}:9090/connections", node_host);
        
        let response = reqwest::get(&url)
            .await
            .with_context(|| format!("Failed to fetch connections from {}", node_host))?;

        if !response.status().is_success() {
            anyhow::bail!("Clash API returned error status: {}", response.status());
        }

        let data: ClashConnectionsResponse = response
            .json()
            .await
            .context("Failed to parse Clash API response")?;

        Ok(data.connections)
    }

    /// Enforce device limit for a single subscription
    async fn enforce_subscription_limit(&self, uuid: &str, active_ips: HashSet<String>) -> Result<()> {
        // Get subscription by UUID
        let sub = match self.get_subscription_by_uuid(uuid).await? {
            Some(s) => s,
            None => {
                warn!("Subscription not found for UUID: {}", uuid);
                return Ok(());
            }
        };

        // Get device limit for this subscription
        let device_limit = self.store.get_subscription_device_limit(sub.id).await?;

        let active_device_count = active_ips.len();
        let ips_vec: Vec<String> = active_ips.iter().cloned().collect();

        // Update IP tracking in database
        self.store.update_subscription_ips(sub.id, ips_vec.clone()).await?;

        // Check if limit exceeded (0 for Unlimited)
        if device_limit > 0 && active_device_count > device_limit as usize {
            warn!(
                "Subscription {} ({}) exceeded device limit: {}/{} devices. Enforcing limit.",
                sub.id, uuid, active_device_count, device_limit
            );

            // Kill all connections for this subscription to enforce re-login
            // A more granular approach would be to kill only the "newest" IP, but we don't have timestamp per connection easily here without storing checks.
            // Killing all forces valid users to maybe reconnect, but kicks the sharers.
            
            self.kill_subscription_connections(uuid).await?;

            // Optional: Send notification via Bot if linked
            // self.orchestration.notify_user(sub.user_id, "Device limit exceeded. Connections reset.").await?;

        } else if active_device_count > 0 {
            info!(
                "Subscription {} ({}) within limit: {}/{} devices",
                sub.id, uuid, active_device_count, if device_limit == 0 { "Unlimited".to_string() } else { device_limit.to_string() }
            );
        }

        Ok(())
    }

    /// Kill all active connections for a specific subscription across all nodes
    pub async fn kill_subscription_connections(&self, uuid: &str) -> Result<()> {
        let nodes = self.orchestration.get_all_nodes().await?;
        
        for node in nodes {
             // 1. Fetch connections again to get IDs (IDs change)
             // Optimization: We could pass the connection list from the check phase if we refactored
             match self.fetch_node_connections(&node.ip).await {
                 Ok(connections) => {
                     for conn in connections {
                         // Check if this connection belongs to the UUID
                         if let Some(conn_uuid) = extract_uuid_from_connection(&conn) {
                             if conn_uuid == uuid {
                                 // Kill it
                                 info!("Killing connection {} on node {} for user {}", conn.id, node.name, uuid);
                                 if let Err(e) = self.close_connection(&node.ip, &conn.id).await {
                                     error!("Failed to close connection {} on {}: {}", conn.id, node.name, e);
                                 }
                             }
                         }
                     }
                 },
                 Err(e) => error!("Failed to fetch connections from {} during kill: {}", node.name, e)
             }
        }
        Ok(())
    }

    /// Close a specific connection on a node via Clash API
    async fn close_connection(&self, node_host: &str, connection_id: &str) -> Result<()> {
        let url = format!("http://{}:9090/connections/{}", node_host, connection_id);
        let client = reqwest::Client::new();
        
        let response = client.delete(&url)
            .send()
            .await
            .with_context(|| format!("Failed to delete connection on {}", node_host))?;
            
        if !response.status().is_success() {
            // 404 means already gone, which is fine
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                 return Ok(());
            }
            anyhow::bail!("Clash API delete error: {}", response.status());
        }
        Ok(())
    }

    /// Get subscription by UUID
    async fn get_subscription_by_uuid(&self, uuid: &str) -> Result<Option<Subscription>> {
        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE uuid = ? LIMIT 1"
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch subscription by UUID")?;

        Ok(sub)
    }
}

/// Extract UUID from connection metadata (inbound tag or chain)
/// This assumes the inbound tag contains the subscription UUID
fn extract_uuid_from_connection(conn: &ClashConnection) -> Option<String> {
    // Check chains for UUID pattern (8-4-4-4-12 hex pattern)
    for chain in &conn.chains {
        if is_valid_uuid(chain) {
            return Some(chain.clone());
        }
    }

    // Fallback: check metadata fields
    // This may need adjustment based on actual sing-box Clash API output
    None
}

/// Simple UUID validation (format check only)
fn is_valid_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }

    parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
}

/// Minimal subscription struct for this service
#[derive(Debug, Clone, sqlx::FromRow)]
struct Subscription {
    #[allow(dead_code)]
    id: i64,
    #[allow(dead_code)]
    user_id: i64,
    #[allow(dead_code)]
    plan_id: i64,
    #[allow(dead_code)]
    uuid: String,
    #[allow(dead_code)]
    traffic_used: i64,
    #[allow(dead_code)]
    traffic_limit: i64,
    #[allow(dead_code)]
    expires_at: chrono::DateTime<Utc>,
    #[allow(dead_code)]
    is_active: bool,
}

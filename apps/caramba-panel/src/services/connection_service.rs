use anyhow::{Context, Result};

use serde::{Deserialize, Serialize};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::time;
use tracing::{error, info, warn};

use crate::services::orchestration_service::OrchestrationService;
use crate::services::store_service::StoreService;
use crate::services::subscription_service::SubscriptionService;

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
    #[serde(default)]
    pub user: Option<String>,
}

/// Clash API response for /connections
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClashConnectionsResponse {
    #[serde(default)]
    pub connections: Vec<ClashConnection>,
}

/// Connection monitoring and device limit enforcement service
pub struct ConnectionService {
    orchestration: Arc<OrchestrationService>,
    store: Arc<StoreService>,
    subscription: Arc<SubscriptionService>,
}

impl ConnectionService {
    pub fn new(
        orchestration: Arc<OrchestrationService>,
        store: Arc<StoreService>,
        subscription: Arc<SubscriptionService>,
    ) -> Self {
        Self {
            orchestration,
            store,
            subscription,
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
            if let Err(e) = self.subscription.cleanup_old_ip_tracking().await {
                error!("Error cleaning up old IP tracking: {:#}", e);
            }
        }
    }

    /// Main enforcement logic: check all nodes and enforce device limits
    async fn check_and_enforce_limits(&self) -> Result<()> {
        info!("Running device limit enforcement cycle...");

        // Get all active nodes
        let nodes: Vec<caramba_db::models::node::Node> = self.orchestration.node_repo.get_all_nodes().await?;
        
        if nodes.is_empty() {
            warn!("No active nodes found, skipping device limit check");
            return Ok(());
        }

        // Collect connections from all nodes
        // Key: Subscription ID (i64), Value: Set of IPs
        let mut subscription_ips: HashMap<i64, HashSet<String>> = HashMap::new();

        // Separate cache for resolving UUID -> SubID to avoid repeatedly DB hitting if using chains
        let mut uuid_cache: HashMap<String, i64> = HashMap::new();

        for node in nodes {
            if node.status != "active" {
                continue;
            }
            match self.fetch_node_connections(&node.ip).await {
                Ok(connections) => {
                    info!("Fetched {} connections from node {}", connections.len(), node.ip);
                    
                    for conn in connections {
                        // Strategy 1: Check metadata.user (e.g. "user_123")
                        let mut sub_id_opt = None;

                        if let Some(user_tag) = &conn.metadata.user {
                            if user_tag.starts_with("user_") {
                                if let Ok(id) = user_tag[5..].parse::<i64>() {
                                    sub_id_opt = Some(id);
                                }
                            }
                        }

                        // Strategy 2: Check chains for UUID if Strategy 1 failed
                        if sub_id_opt.is_none() {
                            if let Some(uuid) = extract_uuid_from_chain(&conn) {
                                if let Some(cached_id) = uuid_cache.get(&uuid) {
                                    sub_id_opt = Some(*cached_id);
                                } else {
                                    // Resolve UUID to ID from DB
                                    if let Ok(Some(sub)) = self.store.get_subscription_by_uuid(&uuid).await {
                                        uuid_cache.insert(uuid.clone(), sub.id);
                                        sub_id_opt = Some(sub.id);
                                    }
                                }
                            }
                        }
                        
                        if let Some(sub_id) = sub_id_opt {
                             subscription_ips
                                .entry(sub_id)
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
        for (sub_id, ips) in subscription_ips {
            if let Err(e) = self.enforce_subscription_limit(sub_id, ips).await {
                error!("Failed to enforce limit for subscription ID {}: {:#}", sub_id, e);
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
    async fn enforce_subscription_limit(&self, sub_id: i64, active_ips: HashSet<String>) -> Result<()> {
        // Get device limit for this subscription
        let device_limit = self.subscription.get_subscription_device_limit(sub_id).await?;

        let active_device_count = active_ips.len();
        let ips_vec: Vec<String> = active_ips.iter().cloned().collect();

        // Update IP tracking in database
        self.store.sub_repo.update_ips(sub_id, ips_vec).await?; 

        // Check if limit exceeded (0 for Unlimited)
        if device_limit > 0 && active_device_count > device_limit as usize {
            warn!(
                "Subscription {} exceeded device limit: {}/{} devices. Enforcing limit.",
                sub_id, active_device_count, device_limit
            );

            // Kill all connections for this subscription
            self.kill_subscription_connections(sub_id).await?;

        } else if active_device_count > 0 {
            info!(
                "Subscription {} within limit: {}/{} devices",
                sub_id, active_device_count, if device_limit == 0 { "Unlimited".to_string() } else { device_limit.to_string() }
            );
        }

        Ok(())
    }

    /// Kill all active connections for a specific subscription across all nodes
    pub async fn kill_subscription_connections(&self, sub_id: i64) -> Result<()> {
        let nodes: Vec<caramba_db::models::node::Node> = self.orchestration.node_repo.get_all_nodes().await?;
        let target_user = format!("user_{}", sub_id);
        
        for node in nodes {
             match self.fetch_node_connections(&node.ip).await {
                 Ok(connections) => {
                     for conn in connections {
                         // Check metadata.user
                         let mut match_found = false;
                         if let Some(user) = &conn.metadata.user {
                             if user == &target_user {
                                 match_found = true;
                             }
                         }
                         
                         if match_found {
                                 info!("Killing connection {} on node {} for {}", conn.id, node.name, target_user);
                                 if let Err(e) = self.close_connection(&node.ip, &conn.id).await {
                                     error!("Failed to close connection {} on {}: {}", conn.id, node.name, e);
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
}

/// Extract UUID from connection chains (legacy support)
fn extract_uuid_from_chain(conn: &ClashConnection) -> Option<String> {
    for chain in &conn.chains {
        if is_valid_uuid(chain) {
            return Some(chain.clone());
        }
    }
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



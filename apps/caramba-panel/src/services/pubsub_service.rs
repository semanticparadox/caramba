use anyhow::Result;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tracing::{error, info, warn};

/// Service to handle Pub/Sub for Long Polling
/// Maintains a list of local waiters (HTTP requests) and a Redis subscriber
#[derive(Debug)]
pub struct PubSubService {
    redis_url: String,
    // Map of "channel_name" -> List of waiters
    waiters: Arc<Mutex<HashMap<String, Vec<oneshot::Sender<String>>>>>,
    redis_client: redis::Client, // For publishing
}

impl PubSubService {
    pub async fn new(redis_url: String) -> Result<Arc<Self>> {
        let client = redis::Client::open(redis_url.clone())?;

        let service = Arc::new(Self {
            redis_url: redis_url.clone(),
            waiters: Arc::new(Mutex::new(HashMap::new())),
            redis_client: client,
        });

        // Spawn background subscriber
        let svc_clone = service.clone();
        tokio::spawn(async move {
            svc_clone.subscription_loop().await;
        });

        Ok(service)
    }

    /// Background loop that listens to Redis PubSub
    async fn subscription_loop(&self) {
        loop {
            info!("🔄 PubSub: Connecting to Redis...");
            match redis::Client::open(self.redis_url.clone()) {
                Ok(client) => {
                    match client.get_async_pubsub().await {
                        Ok(mut pubsub) => {
                            // Subscribe to all node events
                            if let Err(e) = pubsub.psubscribe("node_events:*").await {
                                error!("PubSub Subscribe failed: {}", e);
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                continue;
                            }

                            info!("✅ PubSub: Subscribed to node_events:*");
                            let mut stream = pubsub.on_message();

                            while let Some(msg) = stream.next().await {
                                let msg: redis::Msg = msg;
                                let channel_name = msg.get_channel_name().to_string(); // e.g. node_events:123
                                let payload: String = match msg.get_payload::<String>() {
                                    Ok(s) => s,
                                    Err(_) => continue,
                                };

                                self.notify_waiters(&channel_name, payload);
                            }
                            warn!("PubSub stream ended. Reconnecting...");
                        }
                        Err(e) => {
                            error!("PubSub connection failed: {}", e);
                        }
                    }
                }
                Err(e) => error!("Redis client error: {}", e),
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    fn notify_waiters(&self, channel: &str, payload: String) {
        let mut map = self.waiters.lock().unwrap();
        if let Some(waiters) = map.remove(channel) {
            // tracing::info!("Notify {} waiters on {}", waiters.len(), channel);
            for sender in waiters {
                let _ = sender.send(payload.clone());
            }
        }
    }

    /// Wait for a message on a specific channel (e.g., node_events:123)
    /// Returns a Receiver. The caller must await it.
    pub fn wait_for(&self, channel: &str) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        let mut map = self.waiters.lock().unwrap();
        map.entry(channel.to_string()).or_default().push(tx);
        rx
    }

    /// Publish a message to a channel
    pub async fn publish(&self, channel: &str, message: &str) -> Result<()> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;
        let _: () = redis::cmd("PUBLISH")
            .arg(channel)
            .arg(message)
            .query_async(&mut conn)
            .await?;
        Ok(())
    }
}

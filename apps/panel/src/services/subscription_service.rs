use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::store::{Plan, Subscription, SubscriptionWithDetails, GiftCode, PlanDuration, RenewalResult, SubscriptionIpTracking};
use uuid::Uuid;
use chrono::{Utc, Duration};
use tracing::{info, error, warn};

#[derive(Debug, Clone)]
pub struct SubscriptionService {
    pool: SqlitePool,
}

impl SubscriptionService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_active_plans(&self) -> Result<Vec<Plan>> {
        let mut plans = sqlx::query_as::<_, Plan>(
            "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_active = 1"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active plans")?;

        if plans.is_empty() {
            return Ok(Vec::new());
        }

        let plan_ids: Vec<i64> = plans.iter().map(|p| p.id).collect();
        let plan_ids_str = plan_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let query = format!("SELECT * FROM plan_durations WHERE plan_id IN ({}) ORDER BY duration_days ASC", plan_ids_str);
        
        let all_durations = sqlx::query_as::<_, PlanDuration>(&query)
            .fetch_all(&self.pool)
            .await?;

        for plan in &mut plans {
            plan.durations = all_durations.iter()
                .filter(|d| d.plan_id == plan.id)
                .cloned()
                .collect();
        }

        Ok(plans)
    }

    pub async fn convert_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = ? AND user_id = ?")
            .bind(sub_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .context("Subscription not found")?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!("Only pending subscriptions can be converted to gifts"));
        }

        let duration = sub.expires_at - sub.created_at;
        let duration_days = duration.num_days() as i32;

        sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        let code = format!("EXA-GIFT-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("CODE").to_uppercase());

        sqlx::query(
            "INSERT INTO gift_codes (code, plan_id, duration_days, created_by_user_id) VALUES (?, ?, ?, ?)"
        )
        .bind(&code)
        .bind(sub.plan_id)
        .bind(duration_days)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(code)
    }

    pub async fn redeem_gift_code(&self, user_id: i64, code: &str) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let gift_code_opt = sqlx::query_as::<_, GiftCode>(
            "SELECT * FROM gift_codes WHERE code = ? AND redeemed_by_user_id IS NULL"
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let gift_code = gift_code_opt.ok_or_else(|| anyhow::anyhow!("Invalid or already redeemed code"))?;

        let days = gift_code.duration_days.ok_or_else(|| anyhow::anyhow!("Gift code invalid (no duration)"))?;
        let plan_id = gift_code.plan_id.ok_or_else(|| anyhow::anyhow!("Gift code invalid (no plan)"))?;
        
        let expires_at = Utc::now() + Duration::days(days as i64);
        let vless_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, expires_at, status)
            VALUES (?, ?, ?, ?, 'pending')
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = ?, redeemed_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(user_id)
            .bind(gift_code.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn transfer(&self, sub_id: i64, current_user_id: i64, target_user_id: i64) -> Result<Subscription> {
        let sub = sqlx::query_as::<_, Subscription>(
            "UPDATE subscriptions SET user_id = ? WHERE id = ? AND user_id = ? RETURNING *"
        )
        .bind(target_user_id)
        .bind(sub_id)
        .bind(current_user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(sub)
    }

    pub async fn admin_delete(&self, sub_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete subscription")?;
        Ok(())
    }

    pub async fn admin_extend(&self, sub_id: i64, days: i32) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = datetime(expires_at, '+' || ? || ' days') WHERE id = ?")
            .bind(days)
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to extend subscription")?;
        Ok(())
    }

    pub async fn get_user_subscriptions(&self, user_id: i64) -> Result<Vec<SubscriptionWithDetails>> {
        let subs = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE user_id = ? ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        if subs.is_empty() {
            return Ok(Vec::new());
        }

        let plans = self.get_active_plans().await?; 
        let mut result = Vec::new();

        for sub in subs {
            let plan = plans.iter().find(|p| p.id == sub.plan_id);
            let (name, desc, limit) = if let Some(p) = plan {
                (p.name.clone(), p.description.clone(), Some(p.traffic_limit_gb))
            } else {
                ("Unknown Plan".to_string(), None, None)
            };

            result.push(SubscriptionWithDetails {
                sub,
                plan_name: name,
                plan_description: desc,
                traffic_limit_gb: limit,
            });
        }

        Ok(result)
    }

    pub async fn update_note(&self, sub_id: i64, note: String) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET note = ? WHERE id = ?")
            .bind(note)
            .bind(sub_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_auto_renewal(&self, subscription_id: i64) -> Result<bool> {
        let current: bool = sqlx::query_scalar::<_, Option<i32>>("SELECT auto_renew FROM subscriptions WHERE id = ?")
            .bind(subscription_id)
            .fetch_one(&self.pool)
            .await?
            .map(|v| v != 0)
            .unwrap_or(false);
        
        let new_value = !current;
        sqlx::query("UPDATE subscriptions SET auto_renew = ? WHERE id = ?")
            .bind(new_value as i32)
            .bind(subscription_id)
            .execute(&self.pool)
            .await?;
        
        Ok(new_value)
    }

    pub async fn process_auto_renewals(&self) -> Result<Vec<RenewalResult>> {
        let subs = sqlx::query_as::<_, (i64, i64, i64, String, i64)>(
            "SELECT s.id, s.user_id, s.plan_id, p.name, u.balance 
             FROM subscriptions s
             JOIN users u ON s.user_id = u.id
             JOIN plans p ON s.plan_id = p.id
             WHERE COALESCE(s.auto_renew, 0) = 1
             AND s.status = 'active'
             AND datetime(s.expires_at) BETWEEN datetime('now') AND datetime('now', '+1 day')"
        )
        .fetch_all(&self.pool)
        .await?;
        
        let mut results = vec![];
        for (sub_id, user_id, plan_id, plan_name, balance) in subs {
            let price = sqlx::query_scalar::<_, i64>(
                "SELECT price FROM plan_durations WHERE plan_id = ? ORDER BY duration_days LIMIT 1"
            )
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;
            
            if balance >= price {
                sqlx::query("UPDATE subscriptions SET expires_at = datetime(expires_at, '+30 days') WHERE id = ?")
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;
                
                sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
                    .bind(price)
                    .bind(user_id)
                    .execute(&self.pool)
                    .await?;
                
                results.push(RenewalResult::Success { user_id, sub_id, amount: price, plan_name });
            } else {
                results.push(RenewalResult::InsufficientFunds { user_id, sub_id, required: price, available: balance });
            }
        }
        Ok(results)
    }

    pub async fn get_trial_plan(&self) -> Result<Plan> {
        let mut plan = sqlx::query_as::<_, Plan>(
            "SELECT * FROM plans WHERE COALESCE(is_trial, 0) = 1 AND is_active = 1 LIMIT 1"
        )
        .fetch_one(&self.pool)
        .await
        .context("Trial plan not configured")?;
        
        plan.durations = sqlx::query_as::<_, PlanDuration>("SELECT * FROM plan_durations WHERE plan_id = ?")
            .bind(plan.id)
            .fetch_all(&self.pool)
            .await?;
        
        Ok(plan)
    }

    pub async fn create_trial_subscription(&self, user_id: i64, plan_id: i64, duration_days: i64) -> Result<i64> {
        let sub_id: i64 = sqlx::query_scalar(
            "INSERT INTO subscriptions 
             (user_id, plan_id, status, expires_at, used_traffic, is_trial, created_at, subscription_uuid) 
             VALUES (?, ?, 'active', datetime('now', '+' || ? || ' days'), 0, 1, CURRENT_TIMESTAMP, ?) 
             RETURNING id"
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(duration_days)
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&self.pool)
        .await?;
        
        Ok(sub_id)
    }

    pub async fn get_subscription_links(&self, sub_id: i64) -> Result<Vec<String>> {
        let mut links = Vec::new();
        let sub: Option<Subscription> = sqlx::query_as("SELECT * FROM subscriptions WHERE id = ?")
            .bind(sub_id)
            .fetch_optional(&self.pool)
            .await?;
        
        if let Some(sub) = sub {
            let uuid = sub.vless_uuid.clone().unwrap_or_default();
            let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>(
                r#"
                SELECT i.* FROM inbounds i
                JOIN plan_inbounds pi ON pi.inbound_id = i.id
                WHERE pi.plan_id = ? AND i.enable = 1
                "#
            )
            .bind(sub.plan_id)
            .fetch_all(&self.pool)
            .await?;
            
            for inbound in inbounds {
                use crate::models::network::StreamSettings;
                let stream: StreamSettings = serde_json::from_str(&inbound.stream_settings).unwrap_or_default();
                let security = stream.security.as_deref().unwrap_or("none");
                let network = stream.network.as_deref().unwrap_or("tcp");

                let (address, reality_pub, short_id) = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    let node_details: Option<(String, Option<String>, Option<String>)> = sqlx::query_as("SELECT ip, reality_pub, short_id FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                    if let Some((ip, pub_key, sid)) = node_details { (ip, pub_key, sid) } else { (inbound.listen_ip.clone(), None, None) }
                } else {
                     let node_details: Option<(Option<String>, Option<String>)> = sqlx::query_as("SELECT reality_pub, short_id FROM nodes WHERE id = ?")
                        .bind(inbound.node_id)
                        .fetch_optional(&self.pool)
                        .await?;
                     if let Some((pub_key, sid)) = node_details { (inbound.listen_ip.clone(), pub_key, sid) } else { (inbound.listen_ip.clone(), None, None) }
                };

                let port = inbound.listen_port;
                let remark = inbound.tag.clone();

                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut params = Vec::new();
                        params.push(format!("security={}", security));
                        if security == "reality" {
                            if let Some(reality) = stream.reality_settings {
                                params.push(format!("sni={}", reality.server_names.first().cloned().unwrap_or_default()));
                                params.push(format!("pbk={}", reality_pub.unwrap_or_default())); 
                                if let Some(sid) = &short_id { params.push(format!("sid={}", sid)); }
                                params.push("fp=chrome".to_string());
                            }
                        } else if security == "tls" {
                            if let Some(tls) = stream.tls_settings { params.push(format!("sni={}", tls.server_name)); }
                        }
                        params.push(format!("type={}", network));
                        if network == "tcp" {
                             params.push("headerType=none".to_string());
                             if security == "reality" { params.push("flow=xtls-rprx-vision".to_string()); }
                        }
                        links.push(format!("vless://{}@{}:{}?{}#{}", uuid, address, port, params.join("&"), remark));
                    },
                    "hysteria2" => {
                        let mut params = Vec::new();
                        if let Some(tls) = stream.tls_settings { params.push(format!("sni={}", tls.server_name)); }
                        params.push("insecure=1".to_string());

                        use crate::models::network::InboundType;
                        if let Ok(InboundType::Hysteria2(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            if let Some(obfs) = settings.obfs {
                                if obfs.ttype == "salamander" {
                                    params.push("obfs=salamander".to_string());
                                    params.push(format!("obfs-password={}", obfs.password));
                                }
                            }
                        }

                        let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?").bind(sub.user_id).fetch_optional(&self.pool).await?.unwrap_or(0);
                        let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));
                        links.push(format!("hysteria2://{}@{}:{}?{}#{}", auth, address, port, params.join("&"), remark));
                    },
                    _ => {}
                }
            }
        }
        Ok(links)
    }

    pub async fn get_subscription_device_limit(&self, subscription_id: i64) -> Result<i32> {
        let limit: Option<i32> = sqlx::query_scalar(
            "SELECT p.device_limit FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = ?"
        )
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to fetch device limit")?;
        Ok(limit.unwrap_or(0))
    }

    pub async fn update_ips(&self, subscription_id: i64, ip_list: Vec<String>) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("DELETE FROM subscription_ip_tracking WHERE subscription_id = ?", subscription_id).execute(&mut *tx).await?;
        let now = Utc::now();
        for ip in ip_list {
            sqlx::query!("INSERT INTO subscription_ip_tracking (subscription_id, client_ip, last_seen_at) VALUES (?, ?, ?)", subscription_id, ip, now).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_active_ips(&self, subscription_id: i64) -> Result<Vec<SubscriptionIpTracking>> {
        let cutoff = Utc::now() - Duration::minutes(15);
        sqlx::query_as::<_, SubscriptionIpTracking>(
            "SELECT * FROM subscription_ip_tracking WHERE subscription_id = ? AND last_seen_at > ? ORDER BY last_seen_at DESC"
        )
        .bind(subscription_id)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active IPs")
    }

    pub async fn cleanup_old_ip_tracking(&self) -> Result<u64> {
        let cutoff = Utc::now() - Duration::hours(1);
        let result = sqlx::query("DELETE FROM subscription_ip_tracking WHERE last_seen_at < ?").bind(cutoff).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }
}

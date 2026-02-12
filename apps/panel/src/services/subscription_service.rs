use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::store::{Plan, Subscription, SubscriptionWithDetails, GiftCode, PlanDuration, RenewalResult, SubscriptionIpTracking};
use crate::models::node::Node;
use crate::models::network::InboundType;
use crate::singbox::subscription_generator::{self, NodeInfo, UserKeys};
use uuid::Uuid;
use chrono::{Utc, Duration};


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

    pub async fn admin_gift_subscription(&self, user_id: i64, plan_id: i64, duration_days: i32) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        // 1. Select an active node
        let node_id: i64 = sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active' LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No active nodes available to assign"))?;

        // 2. Prepare subscription data
        let vless_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days as i64);
        let sub_uuid = Uuid::new_v4().to_string();

        // 3. Create Active Subscription
        // Note: sub_uuid logic was in StoreService but maybe implicitly handled or defaulted?
        // Let's ensure we generate it. Subscription struct has it.
        // The original query in store_service::admin_gift_subscription didn't explicitly bind sub_uuid in the snippet I saw!
        // Wait, looking at store_service.rs (line 616), it did NOT bind subscription_uuid!
        // But the schema requires it (or defaults).
        // Let's check `store_service.rs` again carefully.
        // Line 616: INSERT INTO subscriptions (..., subscription_uuid, ...)
        // Wait, the snippet I saw for `admin_gift_subscription` in `store_service` (lines 599-631) 
        // `INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, created_at)`
        // It does NOT insert `subscription_uuid`.
        // If the table column has a default (e.g. generating UUID in SQLite trigger?), then fine.
        // But `create_trial_subscription` (line 280 in `SubscriptionService`) DOES bind it.
        // I should probably bind it to be safe and consistent.
        
        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, subscription_uuid, created_at)
            VALUES (?, ?, ?, ?, ?, 'active', ?, CURRENT_TIMESTAMP)
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access, created_at
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(node_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .bind(sub_uuid)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn get_subscriptions_with_details_for_admin(&self, user_id: i64) -> Result<Vec<crate::models::store::SubscriptionWithPlan>> {
        use crate::models::store::SubscriptionWithPlan;
        let subs = sqlx::query_as::<_, SubscriptionWithPlan>(
            r#"
            SELECT 
                s.id, 
                p.name as plan_name, 
                s.expires_at, 
                s.created_at,
                s.status,
                0 as price, 
                COALESCE(
                    (SELECT COUNT(DISTINCT client_ip) 
                     FROM subscription_ip_tracking 
                     WHERE subscription_id = s.id 
                     AND datetime(last_seen_at) > datetime('now', '-15 minutes')),
                    0
                ) as active_devices,
                p.device_limit as device_limit
            FROM subscriptions s
            JOIN plans p ON s.plan_id = p.id
            WHERE s.user_id = ?
            "#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user subscriptions with details")?;
        
        Ok(subs)
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
                                let sni = reality.server_names.first().cloned().unwrap_or_else(|| address.clone());
                                params.push(format!("sni={}", sni));
                                params.push(format!("pbk={}", reality_pub.unwrap_or_default())); 
                                if let Some(sid) = &short_id { params.push(format!("sid={}", sid)); }
                                params.push("fp=chrome".to_string());
                            }
                        } else if security == "tls" {
                            let sni = stream.tls_settings.as_ref().map(|t| t.server_name.clone()).unwrap_or_else(|| address.clone());
                            params.push(format!("sni={}", sni));
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
                        let sni = stream.tls_settings.as_ref().map(|t| t.server_name.clone()).unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
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
                    "trojan" => {
                        let mut params = Vec::new();
                        params.push("security=tls".to_string());
                        let sni = stream.tls_settings.as_ref().map(|t| t.server_name.clone()).unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("fp=chrome".to_string());
                        params.push(format!("type={}", network));
                        links.push(format!("trojan://{}@{}:{}?{}#{}", uuid, address, port, params.join("&"), remark));
                    },
                    "tuic" => {
                        let mut params = Vec::new();
                        let sni = stream.tls_settings.as_ref().map(|t| t.server_name.clone()).unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("alpn=h3".to_string());
                        
                        let congestion = if let Ok(InboundType::Tuic(settings)) = serde_json::from_str::<InboundType>(&inbound.settings) {
                            settings.congestion_control
                        } else {
                            "cubic".to_string()
                        };
                        params.push(format!("congestion_control={}", congestion));
                        links.push(format!("tuic://{}:{}@{}:{}?{}#{}", uuid, uuid.replace("-", ""), address, port, params.join("&"), remark));
                    },
                    "naive" => {
                        let mut params = Vec::new();
                        let sni = stream.tls_settings.as_ref().map(|t| t.server_name.clone()).unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));

                        if security == "reality" {
                            if let Some(_reality) = stream.reality_settings {
                                params.push(format!("pbk={}", reality_pub.unwrap_or_default()));
                                if let Some(sid) = &short_id { params.push(format!("sid={}", sid)); }
                            }
                        }

                        let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?").bind(sub.user_id).fetch_optional(&self.pool).await?.unwrap_or(0);
                        let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));
                        links.push(format!("naive+https://{}@{}:{}?{}#{}", auth, address, port, params.join("&"), remark));
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

    // --- New methods for Hybrid Frontend support ---

    /// Fetch subscription by its UUID (used for public subscription links)
    pub async fn get_subscription_by_uuid(&self, uuid: &str) -> Result<Subscription> {
        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE subscription_uuid = ?"
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        Ok(sub)
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<Subscription>> {
        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by ID")?;
        Ok(sub)
    }

    /// Helper to convert User-Agent to a readable device name
    pub fn parse_device_name(&self, ua: &str) -> String {
        let ua_lower = ua.to_lowercase();
        
        if ua_lower.contains("iphone") { return "iPhone".to_string(); }
        if ua_lower.contains("ipad") { return "iPad".to_string(); }
        if ua_lower.contains("android") { return "Android Device".to_string(); }
        if ua_lower.contains("windows") { return "Windows PC".to_string(); }
        if ua_lower.contains("macintosh") || ua_lower.contains("mac os x") { return "MacBook/iMac".to_string(); }
        if ua_lower.contains("linux") { return "Linux Device".to_string(); }
        if ua_lower.contains("sing-box") { return "Sing-box Client".to_string(); }
        if ua_lower.contains("clash") { return "Clash Client".to_string(); }
        if ua_lower.contains("v2ray") || ua_lower.contains("xray") { return "Xray/V2Ray Client".to_string(); }
        if ua_lower.contains("streisand") { return "Streisand (iOS)".to_string(); }
        if ua_lower.contains("shadowrocket") { return "Shadowrocket (iOS)".to_string(); }
        if ua_lower.contains("v2box") { return "V2Box".to_string(); }
        
        if ua.len() > 20 {
            format!("{}...", &ua[0..17])
        } else if ua.is_empty() {
            "Unknown Device".to_string()
        } else {
            ua.to_string()
        }
    }

    /// Detects the best client type based on User-Agent
    pub fn detect_client_type(&self, ua: Option<&str>) -> String {
        let ua = match ua {
            Some(s) => s.to_lowercase(),
            None => return "html".to_string(),
        };

        if ua.contains("hiddify") || ua.contains("sing-box") {
            "singbox".to_string()
        } else if ua.contains("clash") || ua.contains("stash") {
            "clash".to_string()
        } else if ua.contains("v2ray") || ua.contains("xray") || ua.contains("fair") || ua.contains("shadowrocket") {
            "v2ray".to_string()
        } else if ua.contains("mozilla") || ua.contains("chrome") || ua.contains("safari") {
            "html".to_string()
        } else {
            // Default for unknown machine clients
            "singbox".to_string()
        }
    }
    /// Update subscription last access time and IP
    pub async fn track_access(&self, sub_id: i64, ip: &str, user_agent: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE subscriptions SET last_sub_access = ?, last_access_ip = ?, last_access_ua = ? WHERE id = ?"
        )
        .bind(Utc::now())
        .bind(ip)
        .bind(user_agent)
        .bind(sub_id)
        .execute(&self.pool)
        .await?;

        // Also log to subscription_ip_tracking
        // We use a simplified tracking here, assuming unique constraint on (sub_id, ip)
        // If user_agent is provided, update it too
        let ua = user_agent.unwrap_or("");
        let device_name = self.parse_device_name(ua);
        
        sqlx::query(
            "INSERT INTO subscription_ip_tracking (subscription_id, client_ip, user_agent, last_seen_at) 
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(subscription_id, client_ip) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP, user_agent = excluded.user_agent"
        )
        .bind(sub_id)
        .bind(ip)
        .bind(device_name) // Store the parsed name or raw? Let's store parsed for now to make UI easier
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fetch all active nodes and convert to NodeInfo for config generation
    pub async fn get_active_nodes_for_config(&self) -> Result<Vec<NodeInfo>> {
        let nodes = sqlx::query_as::<_, Node>(
            "SELECT * FROM nodes WHERE is_enabled = 1 AND status = 'online'"
        )
        .fetch_all(&self.pool)
        .await?;
        
        // Fetch all active inbounds for these nodes
        let node_ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        let inbounds_map = if node_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let ids_str = node_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
            let query = format!("SELECT * FROM inbounds WHERE enable = 1 AND node_id IN ({})", ids_str);
            
            let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>(&query)
                .fetch_all(&self.pool)
                .await?;
                
            let mut map = std::collections::HashMap::new();
            for inbound in inbounds {
                map.entry(inbound.node_id).or_insert_with(Vec::new).push(inbound);
            }
            map
        };

        let node_infos = nodes.iter().map(|n| {
            let node_inbounds = inbounds_map.get(&n.id).cloned().unwrap_or_default();
            NodeInfo::new(n, node_inbounds)
        }).collect();
        
        Ok(node_infos)
    }

    /// Fetch node infos with their associated relay nodes recursively (1 level deep)
    pub async fn get_node_infos_with_relays(&self, nodes: &[Node]) -> Result<Vec<NodeInfo>> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        // 1. Fetch all inbounds for the main nodes
        let node_ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        let inbounds_map = self.fetch_inbounds_for_nodes(&node_ids).await?;

        // 2. Identify and fetch unique relay nodes
        let relay_ids: Vec<i64> = nodes.iter()
            .filter_map(|n| n.relay_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let relays_map = if relay_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let ids_str = relay_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
            let query = format!("SELECT * FROM nodes WHERE id IN ({})", ids_str);
            let relay_nodes = sqlx::query_as::<_, Node>(&query).fetch_all(&self.pool).await?;
            
            // Fetch inbounds for relays too
            let relay_inbounds_map = self.fetch_inbounds_for_nodes(&relay_ids).await?;
            
            let mut map = std::collections::HashMap::new();
            for r in relay_nodes {
                let r_id = r.id;
                let r_inbounds = relay_inbounds_map.get(&r_id).cloned().unwrap_or_default();
                map.insert(r_id, NodeInfo::new(&r, r_inbounds));
            }
            map
        };

        // 3. Build NodeInfo list
        let mut node_infos = Vec::new();
        for n in nodes {
            let n_inbounds = inbounds_map.get(&n.id).cloned().unwrap_or_default();
            let mut ni = NodeInfo::new(n, n_inbounds);
            
            if let Some(r_id) = n.relay_id {
                if let Some(r_info) = relays_map.get(&r_id) {
                    ni.relay_info = Some(Box::new(r_info.clone()));
                }
            }
            node_infos.push(ni);
        }

        Ok(node_infos)
    }

    async fn fetch_inbounds_for_nodes(&self, node_ids: &[i64]) -> Result<std::collections::HashMap<i64, Vec<crate::models::network::Inbound>>> {
        if node_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let ids_str = node_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        let query = format!("SELECT * FROM inbounds WHERE enable = 1 AND node_id IN ({})", ids_str);
        debug_assert!(!ids_str.is_empty());

        let inbounds = sqlx::query_as::<_, crate::models::network::Inbound>(&query)
            .fetch_all(&self.pool)
            .await?;
            
        let mut map = std::collections::HashMap::new();
        for inbound in inbounds {
            map.entry(inbound.node_id).or_insert_with(Vec::new).push(inbound);
        }
        Ok(map)
    }

    /// Get user keys for config generation
    /// Currently uses the subscription's vless_uuid as the user UUID
    pub async fn get_user_keys(&self, sub: &Subscription) -> Result<UserKeys> {
        // We rely on the subscription's UUID for VLESS
        let user_uuid = sub.vless_uuid.clone().ok_or_else(|| anyhow::anyhow!("No VLESS UUID for subscription"))?;
        
        // For Hysteria2, we often need a password. 
        // If not stored, we might generate one or use a consistent hash.
        // For now, let's look closer at `subscription_generator::UserKeys`.
        // It has `hy2_password`. 
        // We can try to fetch it from the user's profile if it exists, or derive it.
        // The existing `get_subscription_links` derives it:
        // `let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?").bind(sub.user_id)...`
        // `let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));`
        
        // Let's replicate this logic to be consistent
        let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = ?")
            .bind(sub.user_id)
            .fetch_optional(&self.pool)
            .await?
            .unwrap_or(0);
            
        // Hysteria2 often uses `user:password` or just `password`.
        // In `get_subscription_links`: `hysteria2://{}@{}:{}...` where auth is `tg_id:uuid_no_dashes`
        // So the "password" for the client might be this auth string? check `subscription_generator`.
        // `subscription_generator` uses `user_keys.hy2_password` in `hysteria2://{password}@{server}...`
        // So `hy2_password` should probably be the full auth string `tg_id:uuid`.
        
        let hy2_password = format!("{}:{}", tg_id, user_uuid.replace("-", ""));

        // Derive AWG private key
        let awg_private_key = self.derive_awg_key(&user_uuid);

        Ok(UserKeys {
            user_uuid,
            hy2_password,
            _awg_private_key: Some(awg_private_key),
        })
    }

    /// Derives a stable X25519 private key from a UUID string
    fn derive_awg_key(&self, uuid: &str) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(uuid.as_bytes());
        hasher.update(b"amneziawg-key-salt");
        let result = hasher.finalize();
        
        let mut key = [0u8; 32];
        key.copy_from_slice(&result[..32]);
        
        // Clamp the key to be a valid X25519 private key
        key[0] &= 248;
        key[31] &= 127;
        key[31] |= 64;
        
        base64::Engine::encode(&base64::prelude::BASE64_STANDARD, key)
    }
    
    // Wrappers for config generation
    pub fn generate_clash(&self, sub: &Subscription, nodes: &[NodeInfo], keys: &UserKeys) -> Result<String> {
        subscription_generator::generate_clash_config(sub, nodes, keys)
    }
    
    pub fn generate_v2ray(&self, sub: &Subscription, nodes: &[NodeInfo], keys: &UserKeys) -> Result<String> {
        subscription_generator::generate_v2ray_config(sub, nodes, keys)
    }
    
    pub fn generate_singbox(&self, sub: &Subscription, nodes: &[NodeInfo], keys: &UserKeys) -> Result<String> {
        subscription_generator::generate_singbox_config(sub, nodes, keys)
    }
}

use crate::services::activity_service::ActivityService;
use crate::singbox::subscription_generator::{NodeInfo, UserKeys};
use anyhow::{Context, Result};
use caramba_db::models::network::InboundType;
use caramba_db::models::node::Node;
use caramba_db::models::store::{
    AlertType, GiftCode, Plan, PlanDuration, RenewalResult, Subscription, SubscriptionIpTracking,
    SubscriptionWithDetails,
};
use caramba_db::repositories::node_repo::NodeRepository;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SubscriptionService {
    pool: PgPool,
}

impl SubscriptionService {
    const INBOUND_SELECT_SQL: &'static str = r#"
        SELECT
            id,
            node_id,
            tag,
            protocol,
            listen_port::BIGINT AS listen_port,
            COALESCE(listen_ip, '::') AS listen_ip,
            COALESCE(settings, '{}') AS settings,
            COALESCE(stream_settings, '{}') AS stream_settings,
            remark,
            COALESCE(enable, TRUE) AS enable,
            COALESCE(renew_interval_mins, 0)::BIGINT AS renew_interval_mins,
            COALESCE(port_range_start, 10000)::BIGINT AS port_range_start,
            COALESCE(port_range_end, 60000)::BIGINT AS port_range_end,
            last_rotated_at,
            created_at
        FROM inbounds
    "#;

    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn is_placeholder_sni(sni: &str) -> bool {
        let sni = sni.trim().to_ascii_lowercase();
        sni.is_empty()
            || sni == "www.google.com"
            || sni == "google.com"
            || sni == "drive.google.com"
    }

    pub async fn get_active_plans(&self) -> Result<Vec<Plan>> {
        let mut plans = sqlx::query_as::<_, Plan>(
            "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_active = TRUE"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active plans")?;

        if plans.is_empty() {
            return Ok(Vec::new());
        }

        let plan_ids: Vec<i64> = plans.iter().map(|p| p.id).collect();
        let query =
            "SELECT * FROM plan_durations WHERE plan_id = ANY($1) ORDER BY duration_days ASC";

        let all_durations = sqlx::query_as::<_, PlanDuration>(&query)
            .bind(&plan_ids)
            .fetch_all(&self.pool)
            .await?;

        for plan in &mut plans {
            plan.durations = all_durations
                .iter()
                .filter(|d| d.plan_id == plan.id)
                .cloned()
                .collect();
        }

        Ok(plans)
    }

    pub async fn convert_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2",
        )
        .bind(sub_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .context("Subscription not found")?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!(
                "Only pending subscriptions can be converted to gifts"
            ));
        }

        let duration = sub.expires_at - sub.created_at;
        let duration_days = duration.num_days() as i32;

        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        let code = format!(
            "CARAMBA-GIFT-{}",
            Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("CODE")
                .to_uppercase()
        );

        sqlx::query(
            "INSERT INTO gift_codes (code, plan_id, duration_days, created_by_user_id) VALUES ($1, $2, $3, $4)"
        )
        .bind(&code)
        .bind(sub.plan_id)
        .bind(duration_days)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Gift Code",
            &format!("Converted sub {} to gift: {}", sub_id, code),
        )
        .await;

        tx.commit().await?;
        Ok(code)
    }

    pub async fn redeem_gift_code(&self, user_id: i64, code: &str) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let gift_code_opt = sqlx::query_as::<_, GiftCode>(
            "SELECT * FROM gift_codes
             WHERE code = $1
               AND redeemed_by_user_id IS NULL
               AND COALESCE(status, 'active') = 'active'
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)",
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let gift_code =
            gift_code_opt.ok_or_else(|| anyhow::anyhow!("Invalid or already redeemed code"))?;

        let days = gift_code
            .duration_days
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no duration)"))?;
        let plan_id = gift_code
            .plan_id
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no plan)"))?;

        let expires_at = Utc::now() + Duration::days(days as i64);
        let vless_uuid = Uuid::new_v4().to_string();
        let subscription_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, expires_at, status, subscription_uuid)
            VALUES ($1, $2, $3, $4, 'pending', $5)
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .bind(subscription_uuid)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = $1, redeemed_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(user_id)
            .bind(gift_code.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn transfer(
        &self,
        sub_id: i64,
        current_user_id: i64,
        target_user_id: i64,
    ) -> Result<Subscription> {
        let sub = sqlx::query_as::<_, Subscription>(
            "UPDATE subscriptions SET user_id = $1 WHERE id = $2 AND user_id = $3 RETURNING *",
        )
        .bind(target_user_id)
        .bind(sub_id)
        .bind(current_user_id)
        .fetch_one(&self.pool)
        .await?;

        let _ = ActivityService::log(
            &self.pool,
            "Transfer",
            &format!(
                "Transferred sub {} from {} to {}",
                sub_id, current_user_id, target_user_id
            ),
        )
        .await;

        Ok(sub)
    }

    pub async fn admin_delete(&self, sub_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete subscription")?;
        Ok(())
    }

    pub async fn admin_extend(&self, sub_id: i64, days: i32) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET expires_at = expires_at + ($1 * interval '1 day') WHERE id = $2")
            .bind(days)
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to extend subscription")?;

        let _ = ActivityService::log(
            &self.pool,
            "Admin Action",
            &format!("Admin extended sub {} by {} days", sub_id, days),
        )
        .await;

        Ok(())
    }

    pub async fn admin_gift_subscription(
        &self,
        user_id: i64,
        plan_id: i64,
        duration_days: i32,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let node_id: i64 =
            sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active' LIMIT 1")
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| anyhow::anyhow!("No active nodes available to assign"))?;

        let vless_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days as i64);
        let sub_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, subscription_uuid, created_at)
            VALUES ($1, $2, $3, $4, $5, 'active', $6, CURRENT_TIMESTAMP)
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

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Admin Action",
            &format!("Admin gifted sub to user {}", user_id),
        )
        .await;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn get_subscriptions_with_details_for_admin(
        &self,
        user_id: i64,
    ) -> Result<Vec<caramba_db::models::store::SubscriptionWithPlan>> {
        let subs = sqlx::query_as::<_, caramba_db::models::store::SubscriptionWithPlan>(
            r#"
            SELECT 
                s.id, 
                p.name as plan_name, 
                COALESCE(s.expires_at, s.created_at, CURRENT_TIMESTAMP) as expires_at, 
                COALESCE(s.created_at, CURRENT_TIMESTAMP) as created_at,
                COALESCE(s.status, 'pending') as status,
                0::bigint as price, 
                COALESCE(
                    (SELECT COUNT(DISTINCT sip.client_ip)
                     FROM subscription_ip_tracking sip
                     WHERE sip.subscription_id = s.id
                     AND sip.last_seen_at > CURRENT_TIMESTAMP - interval '15 minutes'
                     AND sip.client_ip <> '0.0.0.0'
                     AND NOT EXISTS (
                        SELECT 1 FROM nodes n WHERE n.ip = sip.client_ip
                     )),
                    0
                )::bigint as active_devices,
                COALESCE(p.device_limit, 0)::bigint as device_limit
            FROM subscriptions s
            JOIN plans p ON s.plan_id = p.id
            WHERE s.user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user subscriptions with details")?;

        Ok(subs)
    }

    pub async fn get_user_subscriptions(
        &self,
        user_id: i64,
    ) -> Result<Vec<SubscriptionWithDetails>> {
        let subs = sqlx::query_as::<_, Subscription>(
            r#"
            SELECT
                id,
                user_id,
                plan_id,
                node_id,
                vless_uuid,
                COALESCE(expires_at, created_at, CURRENT_TIMESTAMP) AS expires_at,
                COALESCE(status, 'pending') AS status,
                COALESCE(used_traffic, 0)::bigint AS used_traffic,
                traffic_updated_at,
                note,
                COALESCE(auto_renew, FALSE) AS auto_renew,
                COALESCE(alerts_sent, '[]') AS alerts_sent,
                COALESCE(is_trial, FALSE) AS is_trial,
                COALESCE(subscription_uuid, CONCAT('legacy-', id::text)) AS subscription_uuid,
                last_sub_access,
                COALESCE(created_at, CURRENT_TIMESTAMP) AS created_at
            FROM subscriptions
            WHERE user_id = $1
            ORDER BY COALESCE(created_at, CURRENT_TIMESTAMP) DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        if subs.is_empty() {
            return Ok(Vec::new());
        }

        let plans = match self.get_active_plans().await {
            Ok(plans) => plans,
            Err(e) => {
                warn!(
                    "Failed to fetch active plans while building user subscriptions for {}: {}",
                    user_id, e
                );
                Vec::new()
            }
        };
        let mut result = Vec::new();

        for sub in subs {
            let plan = plans.iter().find(|p| p.id == sub.plan_id);
            let (name, desc, limit) = if let Some(p) = plan {
                (
                    p.name.clone(),
                    p.description.clone(),
                    Some(p.traffic_limit_gb),
                )
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
        sqlx::query("UPDATE subscriptions SET note = $1 WHERE id = $2")
            .bind(note)
            .bind(sub_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_auto_renewal(&self, subscription_id: i64) -> Result<bool> {
        let current: Option<bool> = sqlx::query_scalar::<_, Option<bool>>(
            "SELECT auto_renew FROM subscriptions WHERE id = $1",
        )
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await?;

        let new_value = !current.unwrap_or(false);
        sqlx::query("UPDATE subscriptions SET auto_renew = $1 WHERE id = $2")
            .bind(new_value)
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
             WHERE s.auto_renew = TRUE
             AND s.status = 'active'
             AND s.expires_at BETWEEN CURRENT_TIMESTAMP AND CURRENT_TIMESTAMP + interval '1 day'",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = vec![];
        for (sub_id, user_id, plan_id, plan_name, balance) in subs {
            let price = sqlx::query_scalar::<_, i64>(
                "SELECT price FROM plan_durations WHERE plan_id = $1 ORDER BY duration_days LIMIT 1"
            )
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;

            if balance >= price {
                sqlx::query("UPDATE subscriptions SET expires_at = expires_at + interval '30 days' WHERE id = $1")
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;

                sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
                    .bind(price)
                    .bind(user_id)
                    .execute(&self.pool)
                    .await?;

                results.push(RenewalResult::Success {
                    user_id,
                    sub_id,
                    amount: price,
                    plan_name,
                });
            } else {
                results.push(RenewalResult::InsufficientFunds {
                    user_id,
                    sub_id,
                    required: price,
                    available: balance,
                });
            }
        }
        Ok(results)
    }

    pub async fn get_trial_plan(&self) -> Result<Plan> {
        let mut plan = sqlx::query_as::<_, Plan>(
            "SELECT * FROM plans WHERE is_trial = TRUE AND is_active = TRUE LIMIT 1",
        )
        .fetch_one(&self.pool)
        .await
        .context("Trial plan not configured")?;

        plan.durations =
            sqlx::query_as::<_, PlanDuration>("SELECT * FROM plan_durations WHERE plan_id = $1")
                .bind(plan.id)
                .fetch_all(&self.pool)
                .await?;

        Ok(plan)
    }

    pub async fn create_trial_subscription(
        &self,
        user_id: i64,
        plan_id: i64,
        duration_days: i64,
    ) -> Result<i64> {
        let sub_id: i64 = sqlx::query_scalar(
            "INSERT INTO subscriptions 
             (user_id, plan_id, status, expires_at, used_traffic, is_trial, created_at, subscription_uuid) 
             VALUES ($1, $2, 'active', CURRENT_TIMESTAMP + ($3 * interval '1 day'), 0, TRUE, CURRENT_TIMESTAMP, $4) 
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
        let sub: Option<Subscription> = sqlx::query_as("SELECT * FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(sub) = sub {
            let uuid = sub.vless_uuid.clone().unwrap_or_default();
            let inbounds = sqlx::query_as::<_, caramba_db::models::network::Inbound>(&format!(
                r#"
                SELECT DISTINCT i.id,
                       i.node_id,
                       i.tag,
                       i.protocol,
                       i.listen_port::BIGINT AS listen_port,
                       COALESCE(i.listen_ip, '::') AS listen_ip,
                       COALESCE(i.settings, '{{}}') AS settings,
                       COALESCE(i.stream_settings, '{{}}') AS stream_settings,
                       i.remark,
                       COALESCE(i.enable, TRUE) AS enable,
                       COALESCE(i.renew_interval_mins, 0)::BIGINT AS renew_interval_mins,
                       COALESCE(i.port_range_start, 10000)::BIGINT AS port_range_start,
                       COALESCE(i.port_range_end, 60000)::BIGINT AS port_range_end,
                       i.last_rotated_at,
                       i.created_at
                FROM inbounds i
                LEFT JOIN plan_inbounds pi ON pi.inbound_id = i.id
                LEFT JOIN plan_nodes pn ON pn.node_id = i.node_id
                LEFT JOIN node_group_members ngm ON ngm.node_id = i.node_id
                LEFT JOIN plan_groups pg ON pg.group_id = ngm.group_id
                WHERE (pi.plan_id = $1 OR pn.plan_id = $1 OR pg.plan_id = $1) AND i.enable = TRUE
                "#
            ))
            .bind(sub.plan_id)
            .fetch_all(&self.pool)
            .await?;

            for inbound in inbounds {
                use caramba_db::models::network::StreamSettings;
                let stream: StreamSettings =
                    serde_json::from_str(&inbound.stream_settings).unwrap_or_default();
                let security = stream.security.as_deref().unwrap_or("none");
                let network = stream.network.as_deref().unwrap_or("tcp");

                let node_details: Option<(
                    String,
                    Option<String>,
                    Option<String>,
                    String,
                    Option<String>,
                )> = sqlx::query_as(
                    "SELECT ip, reality_pub, short_id, name, reality_sni FROM nodes WHERE id = $1",
                )
                .bind(inbound.node_id)
                .fetch_optional(&self.pool)
                .await?;

                let (node_ip, reality_pub, short_id, node_name, node_reality_sni) =
                    if let Some((ip, pub_key, sid, name, reality_sni)) = node_details {
                        (ip, pub_key, sid, name, reality_sni)
                    } else {
                        (
                            inbound.listen_ip.clone(),
                            None,
                            None,
                            format!("node-{}", inbound.node_id),
                            None,
                        )
                    };

                let address = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    node_ip
                } else {
                    inbound.listen_ip.clone()
                };

                let node_sni = node_reality_sni.filter(|s| !Self::is_placeholder_sni(s));
                let port = inbound.listen_port;
                let protocol_label = inbound.protocol.to_lowercase();
                let transport_label = if network.trim().is_empty() {
                    "tcp".to_string()
                } else {
                    network.to_lowercase()
                };
                let remark = format!(
                    "{}-{} {}-{}",
                    node_name, inbound.node_id, protocol_label, transport_label
                );
                let encoded_remark = urlencoding::encode(&remark).to_string();

                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut params = Vec::new();
                        params.push(format!("security={}", security));
                        if security == "reality" {
                            let inbound_sni = stream
                                .reality_settings
                                .as_ref()
                                .and_then(|reality| reality.server_names.first().cloned())
                                .filter(|s| !Self::is_placeholder_sni(s));
                            let sni = inbound_sni
                                .or_else(|| node_sni.clone())
                                .unwrap_or_else(|| address.clone());
                            params.push(format!("sni={}", sni));
                            params.push(format!("pbk={}", reality_pub.clone().unwrap_or_default()));
                            if let Some(sid) = &short_id {
                                params.push(format!("sid={}", sid));
                            }
                            params.push("fp=chrome".to_string());
                        } else if security == "tls" {
                            let tls_sni = stream
                                .tls_settings
                                .as_ref()
                                .map(|t| t.server_name.clone())
                                .filter(|s| !Self::is_placeholder_sni(s));
                            let sni = tls_sni
                                .or_else(|| node_sni.clone())
                                .unwrap_or_else(|| address.clone());
                            params.push(format!("sni={}", sni));
                        }
                        params.push(format!("type={}", network));
                        if network == "tcp" {
                            params.push("headerType=none".to_string());
                            if security == "reality" {
                                params.push("flow=xtls-rprx-vision".to_string());
                            }
                        }
                        links.push(format!(
                            "vless://{}@{}:{}?{}#{}",
                            uuid,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "hysteria2" => {
                        let mut params = Vec::new();
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("insecure=1".to_string());

                        if let Ok(InboundType::Hysteria2(settings)) =
                            serde_json::from_str::<InboundType>(&inbound.settings)
                        {
                            if let Some(obfs) = settings.obfs {
                                if obfs.ttype == "salamander" {
                                    params.push("obfs=salamander".to_string());
                                    params.push(format!("obfs-password={}", obfs.password));
                                }
                            }
                        }

                        let tg_id: i64 =
                            sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                                .bind(sub.user_id)
                                .fetch_optional(&self.pool)
                                .await?
                                .unwrap_or(0);
                        let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));
                        links.push(format!(
                            "hysteria2://{}@{}:{}?{}#{}",
                            auth,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "trojan" => {
                        let mut params = Vec::new();
                        params.push("security=tls".to_string());
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("fp=chrome".to_string());
                        params.push(format!("type={}", network));
                        links.push(format!(
                            "trojan://{}@{}:{}?{}#{}",
                            uuid,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "tuic" => {
                        let mut params = Vec::new();
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("alpn=h3".to_string());

                        let congestion = if let Ok(InboundType::Tuic(settings)) =
                            serde_json::from_str::<InboundType>(&inbound.settings)
                        {
                            settings.congestion_control
                        } else {
                            "cubic".to_string()
                        };
                        params.push(format!("congestion_control={}", congestion));
                        links.push(format!(
                            "tuic://{}:{}@{}:{}?{}#{}",
                            uuid,
                            uuid.replace("-", ""),
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "naive" => {
                        let mut params = Vec::new();
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));

                        if security == "reality" && stream.reality_settings.is_some() {
                            params.push(format!("pbk={}", reality_pub.clone().unwrap_or_default()));
                            if let Some(sid) = &short_id {
                                params.push(format!("sid={}", sid));
                            }
                        }

                        let tg_id: i64 =
                            sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                                .bind(sub.user_id)
                                .fetch_optional(&self.pool)
                                .await?
                                .unwrap_or(0);
                        let auth = format!("{}:{}", tg_id, uuid.replace("-", ""));
                        links.push(format!(
                            "naive+https://{}@{}:{}?{}#{}",
                            auth,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(links)
    }

    pub async fn get_subscription_device_limit(&self, subscription_id: i64) -> Result<i32> {
        let limit: Option<i32> = sqlx::query_scalar(
            "SELECT p.device_limit FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = $1"
        )
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to fetch device limit")?;
        Ok(limit.unwrap_or(0))
    }

    pub async fn update_ips(&self, subscription_id: i64, ip_list: Vec<String>) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM subscription_ip_tracking WHERE subscription_id = $1")
            .bind(subscription_id)
            .execute(&mut *tx)
            .await?;
        let now = Utc::now();
        for ip in ip_list {
            sqlx::query("INSERT INTO subscription_ip_tracking (subscription_id, client_ip, last_seen_at) VALUES ($1, $2, $3)").bind(subscription_id).bind(ip).bind(now).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_active_ips(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<SubscriptionIpTracking>> {
        let cutoff = Utc::now() - Duration::minutes(15);
        sqlx::query_as::<_, SubscriptionIpTracking>(
            "SELECT sip.*
             FROM subscription_ip_tracking sip
             WHERE sip.subscription_id = $1
               AND sip.last_seen_at > $2
               AND sip.client_ip <> '0.0.0.0'
               AND NOT EXISTS (SELECT 1 FROM nodes n WHERE n.ip = sip.client_ip)
             ORDER BY sip.last_seen_at DESC",
        )
        .bind(subscription_id)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active IPs")
    }

    pub async fn cleanup_old_ip_tracking(&self) -> Result<u64> {
        let cutoff = Utc::now() - Duration::hours(1);
        let result = sqlx::query("DELETE FROM subscription_ip_tracking WHERE last_seen_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn get_subscription_by_uuid(&self, uuid: &str) -> Result<Subscription> {
        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE subscription_uuid = $1",
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        Ok(sub)
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<Subscription>> {
        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by ID")?;
        Ok(sub)
    }

    pub fn parse_device_name(&self, ua: &str) -> String {
        let ua_lower = ua.to_lowercase();

        if ua_lower.contains("iphone") {
            return "iPhone".to_string();
        }
        if ua_lower.contains("ipad") {
            return "iPad".to_string();
        }
        if ua_lower.contains("android") {
            return "Android Device".to_string();
        }
        if ua_lower.contains("windows") {
            return "Windows PC".to_string();
        }
        if ua_lower.contains("macintosh") || ua_lower.contains("mac os x") {
            return "MacBook/iMac".to_string();
        }
        if ua_lower.contains("linux") {
            return "Linux Device".to_string();
        }
        if ua_lower.contains("sing-box") {
            return "Sing-box Client".to_string();
        }
        if ua_lower.contains("clash") {
            return "Clash Client".to_string();
        }
        if ua_lower.contains("v2ray") || ua_lower.contains("xray") {
            return "Xray/V2Ray Client".to_string();
        }
        if ua_lower.contains("streisand") {
            return "Streisand (iOS)".to_string();
        }
        if ua_lower.contains("shadowrocket") {
            return "Shadowrocket (iOS)".to_string();
        }
        if ua_lower.contains("v2box") {
            return "V2Box".to_string();
        }

        if ua.len() > 20 {
            format!("{}...", &ua[0..17])
        } else if ua.is_empty() {
            "Unknown Device".to_string()
        } else {
            ua.to_string()
        }
    }

    pub fn detect_client_type(&self, ua: Option<&str>) -> String {
        let ua = match ua {
            Some(s) => s.to_lowercase(),
            None => return "html".to_string(),
        };

        if ua.contains("hiddify") || ua.contains("sing-box") {
            "singbox".to_string()
        } else if ua.contains("clash") || ua.contains("stash") {
            "clash".to_string()
        } else if ua.contains("v2ray")
            || ua.contains("xray")
            || ua.contains("fair")
            || ua.contains("shadowrocket")
        {
            "v2ray".to_string()
        } else if ua.contains("mozilla") || ua.contains("chrome") || ua.contains("safari") {
            "html".to_string()
        } else {
            "singbox".to_string()
        }
    }

    pub async fn track_access(
        &self,
        sub_id: i64,
        ip: &str,
        user_agent: Option<&str>,
    ) -> Result<()> {
        let normalized_ip = ip.trim();
        if normalized_ip.is_empty() || normalized_ip == "0.0.0.0" || normalized_ip == "::" {
            return Ok(());
        }

        let is_node_ip: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM nodes WHERE ip = $1)")
                .bind(normalized_ip)
                .fetch_one(&self.pool)
                .await
                .unwrap_or(false);
        if is_node_ip {
            // Ignore self/infra requests so they don't pollute device accounting.
            return Ok(());
        }

        sqlx::query(
            "UPDATE subscriptions SET last_sub_access = $1, last_access_ip = $2, last_access_ua = $3 WHERE id = $4"
        )
        .bind(Utc::now())
        .bind(normalized_ip)
        .bind(user_agent)
        .bind(sub_id)
        .execute(&self.pool)
        .await?;

        let ua = user_agent.unwrap_or("");
        let device_name = self.parse_device_name(ua);

        sqlx::query(
            "INSERT INTO subscription_ip_tracking (subscription_id, client_ip, user_agent, last_seen_at) 
             VALUES ($1, $2, $3, CURRENT_TIMESTAMP)
             ON CONFLICT(subscription_id, client_ip) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP, user_agent = excluded.user_agent"
        )
        .bind(sub_id)
        .bind(normalized_ip)
        .bind(device_name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_active_nodes_for_config(&self) -> Result<Vec<NodeInfo>> {
        let node_repo = NodeRepository::new(self.pool.clone());
        let nodes = node_repo
            .get_all_nodes()
            .await?
            .into_iter()
            .filter(|n| n.is_enabled && n.status == "active")
            .collect::<Vec<Node>>();

        let node_ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        let inbounds_map = if node_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let inbounds = sqlx::query_as::<_, caramba_db::models::network::Inbound>(
                "SELECT * FROM inbounds WHERE enable = TRUE AND node_id = ANY($1)",
            )
            .bind(&node_ids)
            .fetch_all(&self.pool)
            .await?;

            let mut map = std::collections::HashMap::new();
            for inbound in inbounds {
                map.entry(inbound.node_id)
                    .or_insert_with(Vec::new)
                    .push(inbound);
            }
            map
        };

        let node_infos = nodes
            .iter()
            .map(|n| {
                let node_inbounds = inbounds_map.get(&n.id).cloned().unwrap_or_default();
                NodeInfo::new(n, node_inbounds)
            })
            .collect();

        Ok(node_infos)
    }

    pub async fn get_node_infos_with_relays(&self, nodes: &[Node]) -> Result<Vec<NodeInfo>> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        let node_ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        let inbounds_map = self.fetch_inbounds_for_nodes(&node_ids).await?;

        let relay_ids: Vec<i64> = nodes
            .iter()
            .filter_map(|n| n.relay_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let relays_map = if relay_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let node_repo = NodeRepository::new(self.pool.clone());
            let mut relay_nodes = Vec::new();
            for relay_id in &relay_ids {
                if let Some(node) = node_repo.get_node_by_id(*relay_id).await? {
                    relay_nodes.push(node);
                }
            }
            let relay_inbounds_map = self.fetch_inbounds_for_nodes(&relay_ids).await?;

            let mut map = std::collections::HashMap::new();
            for r in relay_nodes {
                let r_id = r.id;
                let r_inbounds = relay_inbounds_map.get(&r_id).cloned().unwrap_or_default();
                map.insert(r_id, NodeInfo::new(&r, r_inbounds));
            }
            map
        };

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

    async fn fetch_inbounds_for_nodes(
        &self,
        node_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, Vec<caramba_db::models::network::Inbound>>> {
        if node_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let inbounds = sqlx::query_as::<_, caramba_db::models::network::Inbound>(&format!(
            "{} WHERE enable = TRUE AND node_id = ANY($1)",
            Self::INBOUND_SELECT_SQL
        ))
        .bind(node_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut map = std::collections::HashMap::new();
        for inbound in inbounds {
            map.entry(inbound.node_id)
                .or_insert_with(Vec::new)
                .push(inbound);
        }
        Ok(map)
    }

    pub async fn get_user_keys(&self, sub: &Subscription) -> Result<UserKeys> {
        let user_uuid = sub
            .vless_uuid
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No VLESS UUID for subscription"))?;

        let tg_id: i64 = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
            .bind(sub.user_id)
            .fetch_optional(&self.pool)
            .await?
            .unwrap_or(0);

        let hy2_password = format!("{}:{}", tg_id, user_uuid.replace("-", ""));
        let awg_private_key = self.derive_awg_key(&user_uuid);

        Ok(UserKeys {
            user_uuid,
            hy2_password,
            _awg_private_key: Some(awg_private_key.clone()),
        })
    }

    fn derive_awg_key(&self, uuid: &str) -> String {
        Self::generate_amneziawg_key(uuid)
    }

    pub async fn check_and_send_alerts(&self) -> Result<Vec<(i64, AlertType)>> {
        use caramba_db::models::store::AlertType;
        let mut alerts_to_send = vec![];

        // Traffic alerts (80%, 90%)
        let subs = sqlx::query_as::<_, (i64, i64, i64, String)>(
            "SELECT s.id, s.user_id, s.used_traffic, COALESCE(s.alerts_sent, '[]') 
             FROM subscriptions s
             JOIN plans p ON s.plan_id = p.id
             WHERE s.status = 'active' AND p.traffic_limit_gb > 0",
        )
        .fetch_all(&self.pool)
        .await?;

        for (sub_id, user_id, used_traffic_bytes, alerts_json) in subs {
            // Get traffic limit from plan
            let traffic_limit_gb: i32 = sqlx::query_scalar(
                "SELECT p.traffic_limit_gb FROM plans p
                 JOIN subscriptions s ON s.plan_id = p.id
                 WHERE s.id = $1 LIMIT 1",
            )
            .bind(sub_id)
            .fetch_one(&self.pool)
            .await?;

            if traffic_limit_gb == 0 {
                continue;
            }

            let total_traffic_bytes = traffic_limit_gb as i64 * 1024 * 1024 * 1024;
            let percentage = (used_traffic_bytes as f64 / total_traffic_bytes as f64) * 100.0;

            let mut alerts: Vec<String> = serde_json::from_str(&alerts_json).unwrap_or_default();

            // Check 80% threshold
            if percentage >= 80.0 && !alerts.contains(&"80_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic80));
                alerts.push("80_percent".to_string());
            }

            // Check 90% threshold
            if percentage >= 90.0 && !alerts.contains(&"90_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic90));
                alerts.push("90_percent".to_string());
            }

            // Update alerts_sent
            if !alerts.is_empty() {
                let alerts_json = serde_json::to_string(&alerts)?;
                sqlx::query("UPDATE subscriptions SET alerts_sent = $1 WHERE id = $2")
                    .bind(&alerts_json)
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;
            }
        }

        // Expiry alerts (3 days before)
        let expiring_subs = sqlx::query_as::<_, (i64, String)>(
            "SELECT s.user_id, COALESCE(s.alerts_sent, '[]')
             FROM subscriptions s
             WHERE s.status = 'active'
             AND s.expires_at BETWEEN CURRENT_TIMESTAMP + interval '2 days' AND CURRENT_TIMESTAMP + interval '3 days'"
        )
        .fetch_all(&self.pool)
        .await?;

        for (user_id, alerts_json) in expiring_subs {
            let alerts: Vec<String> = serde_json::from_str(&alerts_json).unwrap_or_default();
            if !alerts.contains(&"expiry_3d".to_string()) {
                alerts_to_send.push((user_id, AlertType::Expiry3Days));
            }
        }

        Ok(alerts_to_send)
    }

    pub fn generate_clash(
        &self,
        sub: &Subscription,
        nodes: &[NodeInfo],
        keys: &UserKeys,
    ) -> Result<String> {
        crate::singbox::subscription_generator::generate_clash_config(sub, nodes, keys)
    }

    pub fn generate_v2ray(
        &self,
        sub: &Subscription,
        nodes: &[NodeInfo],
        keys: &UserKeys,
    ) -> Result<String> {
        crate::singbox::subscription_generator::generate_v2ray_config(sub, nodes, keys)
    }

    pub fn generate_singbox(
        &self,
        sub: &Subscription,
        nodes: &[NodeInfo],
        keys: &UserKeys,
    ) -> Result<String> {
        crate::singbox::subscription_generator::generate_singbox_config(sub, nodes, keys)
    }

    pub async fn update_subscription_node(&self, sub_id: i64, node_id: Option<i64>) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET node_id = $1 WHERE id = $2")
            .bind(node_id)
            .bind(sub_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    fn generate_amneziawg_key(uuid: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(uuid.as_bytes());
        hasher.update(b"amneziawg-key-salt");
        let result = hasher.finalize();

        let mut key = [0u8; 32];
        key.copy_from_slice(&result[..32]);

        key[0] &= 248;
        key[31] &= 127;
        key[31] |= 64;

        base64::Engine::encode(&base64::prelude::BASE64_STANDARD, key)
    }
}

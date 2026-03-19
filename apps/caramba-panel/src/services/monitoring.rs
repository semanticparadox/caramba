use crate::AppState;
use chrono::Utc;
use tokio::time::{Duration, interval};
use tracing::{error, info};

pub struct MonitoringService {
    state: AppState,
}

impl MonitoringService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn start(&self) {
        info!("Starting background monitoring service...");
        let mut interval = interval(Duration::from_secs(30));
        let mut minute_counter = 0;

        loop {
            interval.tick().await;
            minute_counter += 1;

            if let Err(e) = self.check_node_status().await {
                error!("Monitoring error (node status): {}", e);
            }
            if let Err(e) = self.check_frontend_status().await {
                error!("Monitoring error (frontend status): {}", e);
            }

            if minute_counter % 5 == 0 {
                if let Err(e) = self.check_expirations().await {
                    error!("Monitoring error (expirations): {}", e);
                }
                if let Err(e) = self.check_traffic().await {
                    error!("Monitoring error (traffic): {}", e);
                }
            }

            if minute_counter % 60 == 0 {
                if let Err(e) = self.process_auto_renewals().await {
                    error!("Auto-renewal processing error: {}", e);
                }
            }

            if minute_counter % 360 == 0 {
                if let Err(e) = self.check_traffic_alerts().await {
                    error!("Traffic alerts error: {}", e);
                }
                if minute_counter > 10000 {
                    minute_counter = 0;
                }
            }

            if minute_counter % 60 == 0 {
                if let Err(e) = self.check_and_rotate_snis().await {
                    error!("Auto SNI Rotation check error: {}", e);
                }
            }
        }
    }

    async fn check_node_status(&self) -> anyhow::Result<()> {
        // Get names of nodes about to go offline (for admin notification)
        let going_offline: Vec<(String, String)> = sqlx::query_as(
            "SELECT name, country_code FROM nodes WHERE last_seen < CURRENT_TIMESTAMP - INTERVAL '90 seconds' AND status != 'offline' AND status != 'new' AND status != 'disabled'"
        )
        .fetch_all(&self.state.pool)
        .await
        .unwrap_or_default();

        let rows_affected = sqlx::query("UPDATE nodes SET status = 'offline' WHERE last_seen < CURRENT_TIMESTAMP - INTERVAL '90 seconds' AND status != 'offline' AND status != 'new' AND status != 'disabled'")
            .execute(&self.state.pool)
            .await?
            .rows_affected();

        if rows_affected > 0 {
            info!("Marked {} nodes as offline", rows_affected);
            let names: Vec<String> = going_offline.iter().map(|(n, c)| format!("{} ({})", n, c)).collect();
            self.state.bot_manager.notify_admins(
                &self.state.pool,
                &format!("🔴 Nodes went OFFLINE: {}", names.join(", ")),
            ).await;
        }
        Ok(())
    }

    async fn check_frontend_status(&self) -> anyhow::Result<()> {
        let rows_affected = sqlx::query("UPDATE frontend_servers SET status = 'offline' WHERE last_heartbeat < CURRENT_TIMESTAMP - INTERVAL '90 seconds' AND status != 'offline'")
            .execute(&self.state.pool)
            .await?
            .rows_affected();

        if rows_affected > 0 {
            info!("Marked {} frontends as offline", rows_affected);
        }
        Ok(())
    }

    async fn check_expirations(&self) -> anyhow::Result<()> {
        let now = Utc::now();

        let expired_subs: Vec<(i64, i64, Option<i64>)> = sqlx::query_as(
            "SELECT id, user_id, node_id FROM subscriptions WHERE status = 'active' AND expires_at < $1",
        )
        .bind(now)
        .fetch_all(&self.state.pool)
        .await?;

        if expired_subs.is_empty() {
            return Ok(());
        }

        info!(
            "Found {} expired subscriptions. Updating status...",
            expired_subs.len()
        );

        let mut affected_node_ids = std::collections::HashSet::new();

        for (sub_id, user_id, node_id) in &expired_subs {
            sqlx::query("UPDATE subscriptions SET status = 'expired' WHERE id = $1")
                .bind(sub_id)
                .execute(&self.state.pool)
                .await?;

            if let Some(nid) = node_id {
                affected_node_ids.insert(*nid);
            }

            // Notify user that subscription expired
            let tg_id: Option<i64> = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&self.state.pool)
                .await
                .unwrap_or(None);
            if let Some(tg_id) = tg_id {
                let plan_name: String = sqlx::query_scalar(
                    "SELECT COALESCE(p.name, 'Subscription') FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = $1"
                )
                .bind(sub_id)
                .fetch_optional(&self.state.pool)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| "Subscription".to_string());
                let _ = self.state.bot_manager.send_notification(
                    tg_id,
                    &format!("⏰ Your subscription \"{}\" has expired. Renew to keep your VPN access.", plan_name),
                ).await;
            }

            info!(
                "Subscription {} for user {} marked as expired",
                sub_id, user_id
            );
        }

        // Notify affected nodes to regenerate configs (remove expired users)
        for node_id in affected_node_ids {
            let _ = self.state.orchestration_service.notify_node_update(node_id).await;
        }

        Ok(())
    }

    async fn check_traffic(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn process_auto_renewals(&self) -> anyhow::Result<()> {
        use caramba_db::models::store::RenewalResult;

        let results: Vec<RenewalResult> = self
            .state
            .subscription_service
            .process_auto_renewals()
            .await?;

        if results.is_empty() {
            return Ok(());
        }

        info!("Processing {} auto-renewal results", results.len());

        for result in results {
            match result {
                RenewalResult::Success {
                    user_id,
                    sub_id,
                    amount,
                    plan_name,
                } => {
                    if let Ok(Some(user)) =
                        sqlx::query_as::<_, (i64,)>("SELECT tg_id FROM users WHERE id = $1")
                            .bind(user_id)
                            .fetch_optional(&self.state.pool)
                            .await
                    {
                        let msg = format!(
                            "✅ *Auto\\-Renewed\\!*\n\n\
                             💎 Plan: {}\n\
                             💳 Charged: ${:.2}\n\
                             📅 Valid for: 30 days",
                            plan_name.replace("-", "\\-").replace(".", "\\."),
                            amount as f64 / 100.0
                        );

                        let _ = self.state.bot_manager.send_notification(user.0, &msg).await;

                        info!(
                            "Auto-renewed subscription {} for user {}, charged ${:.2}",
                            sub_id,
                            user_id,
                            amount as f64 / 100.0
                        );
                    }
                }
                RenewalResult::InsufficientFunds {
                    user_id,
                    sub_id,
                    required,
                    available,
                } => {
                    if let Ok(Some(user)) =
                        sqlx::query_as::<_, (i64,)>("SELECT tg_id FROM users WHERE id = $1")
                            .bind(user_id)
                            .fetch_optional(&self.state.pool)
                            .await
                    {
                        let msg = format!(
                            "⚠️ *Auto\\-Renewal Failed*\n\n\
                             💰 Balance: ${:.2}\n\
                             💳 Required: ${:.2}\n\n\
                             Please top up your account to renew your subscription\\.",
                            available as f64 / 100.0,
                            required as f64 / 100.0
                        );

                        let _ = self.state.bot_manager.send_notification(user.0, &msg).await;

                        info!(
                            "Auto-renewal failed for sub {} (user {}): insufficient funds",
                            sub_id, user_id
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn check_traffic_alerts(&self) -> anyhow::Result<()> {
        use caramba_db::models::store::AlertType;

        let alerts: Vec<(i64, AlertType)> = self
            .state
            .subscription_service
            .check_and_send_alerts()
            .await?;

        if alerts.is_empty() {
            return Ok(());
        }

        info!("Sending {} traffic alerts", alerts.len());

        for (user_id, alert_type) in alerts {
            if let Ok(Some(user)) =
                sqlx::query_as::<_, (i64,)>("SELECT tg_id FROM users WHERE id = $1")
                    .bind(user_id)
                    .fetch_optional(&self.state.pool)
                    .await
            {
                let msg = match alert_type {
                    AlertType::Traffic80 => {
                        "⚠️ *Traffic Alert*\n\n\
                         You've used *80%* of your traffic\\.\n\
                         Consider upgrading your plan or topping up\\."
                    }
                    AlertType::Traffic90 => {
                        "⚠️ *Traffic Alert*\n\n\
                         You've used *90%* of your traffic\\.\n\
                         _Service will be paused at 100%\\._"
                    }
                    AlertType::Expiry3Days => {
                        "⏰ *Expiry Alert*\n\n\
                         Your subscription expires in *3 days*\\.\n\
                         Renew now to avoid interruption\\."
                    }
                };

                let _ = self.state.bot_manager.send_notification(user.0, msg).await;
            }
        }

        Ok(())
    }

    async fn check_and_rotate_snis(&self) -> anyhow::Result<()> {
        // Глобальный интервал ротации — применяется к нодам без явной настройки
        let global_interval_hours = self
            .state
            .settings
            .get_or_default("auto_sni_rotation_interval_hours", "24")
            .await
            .parse::<i64>()
            .unwrap_or(24);

        // Если глобальный интервал <= 0, ротируем только ноды с явным per-node интервалом > 0
        let now = Utc::now();

        // Выбираем ноды с учётом per-node интервала:
        //   - sni_renew_interval_hours IS NULL → используем глобальный интервал (если > 0)
        //   - sni_renew_interval_hours = 0    → автоматическая ротация ОТКЛЮЧЕНА, пропускаем
        //   - sni_renew_interval_hours > 0    → используем per-node интервал
        let nodes_to_rotate: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT id FROM nodes
            WHERE is_enabled = TRUE
              AND (
                -- Per-node интервал задан явно (> 0): проверяем по нему
                (sni_renew_interval_hours > 0
                    AND (last_sni_rotation IS NULL
                         OR last_sni_rotation < NOW() - (sni_renew_interval_hours * INTERVAL '1 hour')))
                OR
                -- Нода использует глобальный интервал (NULL), глобальный > 0
                (sni_renew_interval_hours IS NULL AND $1 > 0
                    AND (last_sni_rotation IS NULL
                         OR last_sni_rotation < NOW() - ($1 * INTERVAL '1 hour')))
              )
            "#,
        )
        .bind(global_interval_hours)
        .fetch_all(&self.state.pool)
        .await?;

        if !nodes_to_rotate.is_empty() {
            info!(
                "Found {} nodes due for routine SNI rotation (global interval: {}h)",
                nodes_to_rotate.len(),
                global_interval_hours
            );

            for node_id in nodes_to_rotate {
                match self
                    .state
                    .security_service
                    .rotate_node_sni(node_id, "Routine Global Rotation")
                    .await
                {
                    Ok((old_sni, new_sni, _log_id)) => {
                        info!(
                            "🔄 Routine Rotation: Node {} switched from {} to {}",
                            node_id, old_sni, new_sni
                        );

                        // Обновляем метку последней ротации
                        if let Err(e) = sqlx::query(
                            "UPDATE nodes SET last_sni_rotation = $1 WHERE id = $2",
                        )
                        .bind(now)
                        .bind(node_id)
                        .execute(&self.state.pool)
                        .await
                        {
                            error!(
                                "Failed to update last_sni_rotation for node {}: {}",
                                node_id, e
                            );
                        }

                        if let Err(e) = self
                            .state
                            .pubsub
                            .publish(&format!("node_events:{}", node_id), "sni_update")
                            .await
                        {
                            error!(
                                "Failed to signal node {} for routine SNI rotation: {}",
                                node_id, e
                            );
                        } else {
                            info!("⚡ Signaled node {} to apply routine SNI rotation", node_id);
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed routine SNI rotation for node {}: {}", node_id, e);
                    }
                }
            }
        }
        Ok(())
    }
}

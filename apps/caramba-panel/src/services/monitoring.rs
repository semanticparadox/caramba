use crate::AppState;
use crate::bot::translations::{lang_for, t, tf};
use crate::bot::utils::escape_html;
use chrono::Utc;
use sqlx::PgPool;
use tokio::time::{Duration, interval};
use tracing::{error, info, warn};

pub struct MonitoringService {
    state: AppState,
}

impl MonitoringService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Record a task error and page admins ONCE when consecutive failures
    /// cross the alert threshold (currently 3 — see task_health::ALERT_THRESHOLD).
    /// Single transient errors are silent; sustained failure escalates.
    async fn record_error(&self, name: &str, err: &str) {
        if let Some(count) = self.state.task_health.record_error(name, err).await {
            let alert = format!(
                "⚠️ *Background task is failing*\n\nTask: `{}`\nConsecutive errors: {}\nLast error: `{}`\n\nCheck panel logs and the System Health admin tab.",
                name, count, err
            );
            self.state
                .bot_manager
                .notify_admins(&self.state.pool, &alert)
                .await;
        }
    }

    pub async fn start(&self) {
        info!("Starting background monitoring service...");
        let mut interval = interval(Duration::from_secs(30));
        let mut minute_counter = 0;

        loop {
            interval.tick().await;
            minute_counter += 1;

            match self.check_node_status().await {
                Ok(_) => {
                    self.state
                        .task_health
                        .record_success("check_node_status")
                        .await
                }
                Err(e) => {
                    error!("Monitoring error (node status): {}", e);
                    self.record_error("check_node_status", &e.to_string()).await;
                }
            }
            match self.check_frontend_status().await {
                Ok(_) => {
                    self.state
                        .task_health
                        .record_success("check_frontend_status")
                        .await
                }
                Err(e) => {
                    error!("Monitoring error (frontend status): {}", e);
                    self.record_error("check_frontend_status", &e.to_string())
                        .await;
                }
            }

            // Payment polling fallback (U20/U13): every 4 ticks (~2 min) poll
            // recent still-pending sessions and fulfill any the provider now
            // reports as paid — recovers payments whose webhook was lost.
            if minute_counter % 4 == 0 {
                // Window: poll sessions created in the last 24h (matches the
                // stale-expiry horizon); cap the batch so one tick stays cheap.
                match self
                    .state
                    .marketplace_service
                    .poll_pending_sessions(24, 200)
                    .await
                {
                    Ok(n) => {
                        self.state
                            .task_health
                            .record_success("poll_pending_payments")
                            .await;
                        if n > 0 {
                            info!(
                                "poll_pending_payments: fulfilled {} session(s) via polling fallback",
                                n
                            );
                        }
                    }
                    Err(e) => {
                        error!("poll_pending_payments error: {}", e);
                        self.record_error("poll_pending_payments", &e.to_string())
                            .await;
                    }
                }
            }

            if minute_counter % 5 == 0 {
                match self.check_expirations().await {
                    Ok(_) => {
                        self.state
                            .task_health
                            .record_success("check_expirations")
                            .await
                    }
                    Err(e) => {
                        error!("Monitoring error (expirations): {}", e);
                        self.record_error("check_expirations", &e.to_string()).await;
                    }
                }
                match self.check_traffic().await {
                    Ok(_) => self.state.task_health.record_success("check_traffic").await,
                    Err(e) => {
                        error!("Monitoring error (traffic): {}", e);
                        self.record_error("check_traffic", &e.to_string()).await;
                    }
                }
            }

            if minute_counter % 60 == 0 {
                match self.process_auto_renewals().await {
                    Ok(_) => {
                        self.state
                            .task_health
                            .record_success("process_auto_renewals")
                            .await
                    }
                    Err(e) => {
                        error!("Auto-renewal processing error: {}", e);
                        self.record_error("process_auto_renewals", &e.to_string())
                            .await;
                    }
                }
                match self.check_low_balances().await {
                    Ok(_) => {
                        self.state
                            .task_health
                            .record_success("check_low_balances")
                            .await
                    }
                    Err(e) => {
                        error!("Low balance check error: {}", e);
                        self.record_error("check_low_balances", &e.to_string())
                            .await;
                    }
                }
            }

            // Каждые 60 тиков (30 минут при тике 30 сек): суточное пополнение трафика.
            // Пополнение делается на daily_traffic_mb МБ — восстанавливает доступ
            // пользователям бесплатного плана, исчерпавшим дневной лимит.
            if minute_counter % 60 == 0 {
                match daily_traffic_topup(&self.state.pool).await {
                    Ok(reactivated_plans) => {
                        // Возвращённые из 'throttled' подписки снова должны
                        // попасть в конфиги — регенерируем ноды этих планов.
                        if !reactivated_plans.is_empty()
                            && let Err(e) = self
                                .state
                                .orchestration_service
                                .notify_nodes_for_plans(&reactivated_plans)
                                .await
                        {
                            error!(
                                "Failed to notify nodes after daily top-up reactivation (plans {:?}): {}",
                                reactivated_plans, e
                            );
                        }
                        self.state
                            .task_health
                            .record_success("daily_traffic_topup")
                            .await
                    }
                    Err(e) => {
                        error!("Daily traffic top-up error: {}", e);
                        self.record_error("daily_traffic_topup", &e.to_string())
                            .await;
                    }
                }
            }

            if minute_counter % 360 == 0 {
                match self.check_traffic_alerts().await {
                    Ok(_) => {
                        self.state
                            .task_health
                            .record_success("check_traffic_alerts")
                            .await
                    }
                    Err(e) => {
                        error!("Traffic alerts error: {}", e);
                        self.record_error("check_traffic_alerts", &e.to_string())
                            .await;
                    }
                }
                if minute_counter > 10000 {
                    minute_counter = 0;
                }
            }

            // Каждые 2880 тиков (24 часа при тике 30 сек): автоматическое закрытие
            // тикетов, ожидающих ответа пользователя больше 7 дней.
            if minute_counter % 2880 == 0 {
                match self.state.tickets_svc.auto_close_stale().await {
                    Ok(n) => {
                        self.state
                            .task_health
                            .record_success("auto_close_stale_tickets")
                            .await;
                        if n > 0 {
                            info!("auto_close_stale_tickets: закрыто {} тикетов", n);
                        }
                    }
                    Err(e) => {
                        error!("auto_close_stale_tickets error: {}", e);
                        self.record_error("auto_close_stale_tickets", &e.to_string())
                            .await;
                    }
                }

                // Same daily slot: cancel payment_sessions that have been pending
                // for more than 24h. Without this, abandoned checkouts pile up
                // forever (no active poll, no webhook would fix them).
                match self
                    .state
                    .marketplace_service
                    .expire_stale_sessions(24)
                    .await
                {
                    Ok(_) => {
                        self.state
                            .task_health
                            .record_success("expire_stale_payment_sessions")
                            .await;
                    }
                    Err(e) => {
                        error!("expire_stale_payment_sessions error: {}", e);
                        self.record_error("expire_stale_payment_sessions", &e.to_string())
                            .await;
                    }
                }

                // Same daily slot: payment reconciliation audit (U21). Read-only —
                // scans the last 24h of sessions for divergence and pages admins
                // with any findings. No auto-mutation: a human decides remediation.
                match self.state.marketplace_service.reconcile_recent(24).await {
                    Ok(findings) => {
                        self.state
                            .task_health
                            .record_success("reconcile_payments")
                            .await;
                        if !findings.is_empty() {
                            let body = findings
                                .iter()
                                .map(|f| format!("• {}", f))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let alert = format!(
                                "🧾 *Payment reconciliation found {} issue(s)*\n\n{}\n\nReview in the panel — no automatic changes were made.",
                                findings.len(),
                                body
                            );
                            self.state
                                .bot_manager
                                .notify_admins(&self.state.pool, &alert)
                                .await;
                        }
                    }
                    Err(e) => {
                        error!("reconcile_payments error: {}", e);
                        self.record_error("reconcile_payments", &e.to_string())
                            .await;
                    }
                }

                // Ежесуточная резервная копия БД + ротация по последним 14 копиям.
                // Число хранимых копий можно переопределить через env BACKUP_KEEP (default 14).
                self.run_daily_backup().await;
            }

            if minute_counter % 60 == 0 {
                match self.check_and_rotate_snis().await {
                    Ok(_) => {
                        self.state
                            .task_health
                            .record_success("check_and_rotate_snis")
                            .await
                    }
                    Err(e) => {
                        error!("Auto SNI Rotation check error: {}", e);
                        self.record_error("check_and_rotate_snis", &e.to_string())
                            .await;
                    }
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
            let names: Vec<String> = going_offline
                .iter()
                .map(|(n, c)| format!("{} ({})", n, c))
                .collect();
            self.state
                .bot_manager
                .notify_admins(
                    &self.state.pool,
                    &format!("🔴 Nodes went OFFLINE: {}", names.join(", ")),
                )
                .await;
        }
        Ok(())
    }

    async fn check_frontend_status(&self) -> anyhow::Result<()> {
        // Snapshot which frontends are about to flip to offline so we can alert
        // by hostname, not just by row-count.
        let going_offline: Vec<(String, String)> = sqlx::query_as(
            "SELECT domain, region FROM frontend_servers \
             WHERE last_heartbeat < CURRENT_TIMESTAMP - INTERVAL '90 seconds' AND status != 'offline'"
        )
        .fetch_all(&self.state.pool)
        .await
        .unwrap_or_default();

        let rows_affected = sqlx::query("UPDATE frontend_servers SET status = 'offline' WHERE last_heartbeat < CURRENT_TIMESTAMP - INTERVAL '90 seconds' AND status != 'offline'")
            .execute(&self.state.pool)
            .await?
            .rows_affected();

        if rows_affected > 0 {
            info!("Marked {} frontends as offline", rows_affected);
            let hosts: Vec<String> = going_offline
                .iter()
                .map(|(domain, region)| format!("{} [{}]", domain, region))
                .collect();
            self.state
                .bot_manager
                .notify_admins(
                    &self.state.pool,
                    &format!("🟠 Frontend(s) went OFFLINE: {}", hosts.join(", ")),
                )
                .await;
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
        let mut free_plans_to_regen: std::collections::HashSet<i64> =
            std::collections::HashSet::new();

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
                let lang = crate::bot::utils::lang_by_tg_id(&self.state, tg_id).await;
                let plan_name: String = sqlx::query_scalar(
                    "SELECT COALESCE(p.name, 'Subscription') FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = $1"
                )
                .bind(sub_id)
                .fetch_optional(&self.state.pool)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| t(lang, "label_subscription").to_string());
                // Plain text — без разметки, поэтому имя тарифа не экранируем.
                let payload = crate::bot_manager::NotificationPayload::plain(tf(
                    lang,
                    "notify.expired",
                    &[&plan_name],
                ));
                let _ = self
                    .state
                    .bot_manager
                    .send_rich_notification(tg_id, payload)
                    .await;
                let _ = self
                    .state
                    .notifications_svc
                    .create_inbox_only(
                        *user_id,
                        "subscription",
                        "warning",
                        "Subscription expired",
                        &format!(
                            "Your \"{}\" subscription has expired. Renew to keep your VPN access.",
                            plan_name
                        ),
                        Some(serde_json::json!({"sub_id": sub_id, "url": "/billing"})),
                    )
                    .await;
            }

            info!(
                "Subscription {} for user {} marked as expired",
                sub_id, user_id
            );

            // Fallback: leave the user on the free plan so they can still
            // connect and reach the payment screen. Without this an expiring
            // paid user drops out of every node config and cannot renew from
            // inside the app. Best-effort — a failure here must not abort the
            // expiry sweep for the remaining subscriptions.
            match self
                .state
                .store_service
                .ensure_free_plan_subscription(*user_id)
                .await
            {
                Ok(Some(plan_id)) => {
                    free_plans_to_regen.insert(plan_id);
                }
                Ok(None) => {}
                Err(e) => {
                    error!(
                        "Failed to restore the free plan for user {} after expiry: {}",
                        user_id, e
                    );
                }
            }
        }

        // Notify affected nodes to regenerate configs (remove expired users)
        for node_id in affected_node_ids {
            let _ = self
                .state
                .orchestration_service
                .notify_node_update(node_id)
                .await;
        }

        // …and regenerate for the free plan too, otherwise the fallback
        // subscription exists in the DB but not in any node's config, which
        // looks to the user exactly like having no access at all.
        if !free_plans_to_regen.is_empty() {
            let plan_ids: Vec<i64> = free_plans_to_regen.into_iter().collect();
            if let Err(e) = self
                .state
                .orchestration_service
                .notify_nodes_for_plans(&plan_ids)
                .await
            {
                error!(
                    "Failed to publish configs for the free-plan fallback (plans {:?}): {}",
                    plan_ids, e
                );
            }
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
                    // Получаем tg_id и язык пользователя одним запросом
                    let user_row = sqlx::query_as::<_, (i64, Option<String>)>(
                        "SELECT tg_id, language_code FROM users WHERE id = $1",
                    )
                    .bind(user_id)
                    .fetch_optional(&self.state.pool)
                    .await;

                    if let Ok(Some((tg_id, lang))) = user_row {
                        // Получаем новую дату истечения после продления
                        let new_expires: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
                            "SELECT expires_at FROM subscriptions WHERE id = $1",
                        )
                        .bind(sub_id)
                        .fetch_optional(&self.state.pool)
                        .await
                        .unwrap_or(None);

                        let expires_str = new_expires
                            .map(|dt| dt.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "N/A".to_string());

                        let lang = lang_for(&self.state.settings, lang.as_deref()).await;
                        let amount_str = format!("{:.2}", amount as f64 / 100.0);

                        // Сообщение уходит в HTML parse mode — экранируем под HTML.
                        let msg = tf(
                            lang,
                            "notify.renewed",
                            &[
                                &escape_html(&plan_name),
                                &escape_html(&expires_str),
                                &amount_str,
                            ],
                        );

                        let _ = self
                            .state
                            .bot_manager
                            .send_rich_notification(
                                tg_id,
                                crate::bot_manager::NotificationPayload::html(msg),
                            )
                            .await;
                        let _ = self
                            .state
                            .notifications_svc
                            .create_inbox_only(
                                user_id,
                                "subscription",
                                "info",
                                t(lang, "notify.renewed_title"),
                                // Инбокс Mini App показывает текст как есть — без разметки.
                                &tf(
                                    lang,
                                    "notify.renewed_body",
                                    &[&plan_name, &amount_str, &expires_str],
                                ),
                                Some(serde_json::json!({"sub_id": sub_id, "url": "/subscription"})),
                            )
                            .await;

                        info!(
                            "Auto-renewed subscription {} for user {}, charged ${:.2}, expires {}",
                            sub_id,
                            user_id,
                            amount as f64 / 100.0,
                            expires_str
                        );
                    }
                }
                RenewalResult::InsufficientFunds {
                    user_id,
                    sub_id,
                    required,
                    available,
                } => {
                    let user_row = sqlx::query_as::<_, (i64, Option<String>)>(
                        "SELECT tg_id, language_code FROM users WHERE id = $1",
                    )
                    .bind(user_id)
                    .fetch_optional(&self.state.pool)
                    .await;

                    if let Ok(Some((tg_id, lang))) = user_row {
                        // Получаем имя плана для информативного сообщения
                        let plan_name: String = sqlx::query_scalar(
                            "SELECT COALESCE(p.name, 'Subscription') FROM subscriptions s \
                             JOIN plans p ON s.plan_id = p.id WHERE s.id = $1",
                        )
                        .bind(sub_id)
                        .fetch_optional(&self.state.pool)
                        .await
                        .unwrap_or(None)
                        .unwrap_or_else(|| "Subscription".to_string());

                        let lang = lang_for(&self.state.settings, lang.as_deref()).await;
                        let plan_name = if plan_name == "Subscription" {
                            t(lang, "label_subscription").to_string()
                        } else {
                            plan_name
                        };
                        let avail_str = format!("{:.2}", available as f64 / 100.0);
                        let req_str = format!("{:.2}", required as f64 / 100.0);

                        let msg = tf(
                            lang,
                            "notify.renew_failed",
                            &[&escape_html(&plan_name), &avail_str, &req_str],
                        );

                        let _ = self
                            .state
                            .bot_manager
                            .send_rich_notification(
                                tg_id,
                                crate::bot_manager::NotificationPayload::html(msg),
                            )
                            .await;
                        let _ = self
                            .state
                            .notifications_svc
                            .create_inbox_only(
                                user_id,
                                "subscription",
                                "error",
                                t(lang, "notify.renew_failed_title"),
                                &tf(
                                    lang,
                                    "notify.renew_failed_body",
                                    &[&plan_name, &avail_str, &req_str],
                                ),
                                Some(serde_json::json!({"sub_id": sub_id, "url": "/billing"})),
                            )
                            .await;

                        warn!(
                            "Auto-renewal failed for sub {} (user {}): balance={} required={}",
                            sub_id, user_id, available, required
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Проверяет пользователей с активными подписками (auto_renew=true) и балансом < 100 центов.
    /// Использует Redis-ключ `balance_warned:{user_id}` с TTL 24ч для дедупликации —
    /// не спамим пользователю больше одного предупреждения в сутки.
    async fn check_low_balances(&self) -> anyhow::Result<()> {
        // Выбираем пользователей с авто-продлением и низким балансом
        let low_balance_users: Vec<(i64, i64, Option<String>, String)> = sqlx::query_as(
            r#"
            SELECT DISTINCT u.id, u.tg_id, u.language_code, p.name
            FROM users u
            JOIN subscriptions s ON s.user_id = u.id
            JOIN plans p ON s.plan_id = p.id
            WHERE s.status = 'active'
              AND s.auto_renew = TRUE
              AND u.balance < 100
            ORDER BY u.id
            "#,
        )
        .fetch_all(&self.state.pool)
        .await?;

        if low_balance_users.is_empty() {
            return Ok(());
        }

        info!(
            "Found {} users with low balance and active auto-renew subscriptions",
            low_balance_users.len()
        );

        for (user_db_id, tg_id, lang, plan_name) in low_balance_users {
            let redis_key = format!("balance_warned:{}", user_db_id);

            // Проверяем дедупликацию: уже предупреждали за последние 24 часа?
            let already_warned = self.state.redis.exists(&redis_key).await.unwrap_or(false);

            if already_warned {
                continue;
            }

            // Получаем актуальный баланс для отображения пользователю
            let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE id = $1")
                .bind(user_db_id)
                .fetch_optional(&self.state.pool)
                .await
                .unwrap_or(None)
                .unwrap_or(0);

            let balance_str = format!("{:.2}", balance as f64 / 100.0);
            let lang = lang_for(&self.state.settings, lang.as_deref()).await;

            let msg = tf(
                lang,
                "notify.low_balance",
                &[&balance_str, &escape_html(&plan_name)],
            );

            let _ = self
                .state
                .bot_manager
                .send_rich_notification(tg_id, crate::bot_manager::NotificationPayload::html(msg))
                .await;
            let _ = self
                .state
                .notifications_svc
                .create_inbox_only(
                    user_db_id,
                    "payment",
                    "warning",
                    t(lang, "notify.low_balance_title"),
                    &tf(lang, "notify.low_balance_body", &[&balance_str, &plan_name]),
                    Some(serde_json::json!({"url": "/billing"})),
                )
                .await;

            // Ставим флаг в Redis на 24 часа — не беспокоим снова до следующих суток
            if let Err(e) = self.state.redis.set(&redis_key, "1", 86400).await {
                error!(
                    "Failed to set balance_warned Redis key for user {}: {}",
                    user_db_id, e
                );
            }

            info!(
                "Sent low balance warning to user {} (tg_id={}), balance=${:.2}",
                user_db_id,
                tg_id,
                balance as f64 / 100.0
            );
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
            if let Ok(Some(user)) = sqlx::query_as::<_, (i64, Option<String>)>(
                "SELECT tg_id, language_code FROM users WHERE id = $1",
            )
            .bind(user_id)
            .fetch_optional(&self.state.pool)
            .await
            {
                let lang = lang_for(&self.state.settings, user.1.as_deref()).await;

                // Ключи одинаковой формы: <base>, <base>_title, <base>_body.
                let (base, severity, category) = match alert_type {
                    AlertType::Traffic80 => ("notify.traffic80", "warning", "subscription"),
                    AlertType::Traffic90 => ("notify.traffic90", "warning", "subscription"),
                    AlertType::TrafficExceeded => {
                        ("notify.traffic_exceeded", "error", "subscription")
                    }
                    AlertType::Expiry3Days => ("notify.expiry3", "warning", "subscription"),
                };

                let _ = self
                    .state
                    .bot_manager
                    .send_rich_notification(
                        user.0,
                        crate::bot_manager::NotificationPayload::html(t(lang, base)),
                    )
                    .await;
                let _ = self
                    .state
                    .notifications_svc
                    .create_inbox_only(
                        user_id,
                        category,
                        severity,
                        t(lang, &format!("{base}_title")),
                        t(lang, &format!("{base}_body")),
                        Some(serde_json::json!({"url": "/subscription"})),
                    )
                    .await;
            }
        }

        Ok(())
    }

    /// Создаёт ежесуточную резервную копию базы данных и ротирует старые файлы.
    ///
    /// Количество хранимых копий задаётся через env BACKUP_KEEP (по умолчанию 14).
    /// Результат записывается в task_health registry как "daily_db_backup".
    /// При ошибке уведомляет администраторов через bot_manager.
    async fn run_daily_backup(&self) {
        use crate::services::backup_service;

        info!("Starting daily DB backup...");
        match backup_service::create_backup().await {
            Ok(info) => {
                self.state
                    .task_health
                    .record_success("daily_db_backup")
                    .await;
                info!(
                    filename = %info.filename,
                    size_bytes = info.size_bytes,
                    duration_ms = info.duration_ms,
                    "Daily DB backup completed"
                );

                // Ротация — удаляем файлы сверх лимита
                let keep: usize = std::env::var("BACKUP_KEEP")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(14);

                match backup_service::rotate(keep).await {
                    Ok(deleted) if deleted > 0 => {
                        info!("Backup rotation: удалено {} старых файлов", deleted);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Backup rotation error: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Daily DB backup failed: {}", e);
                let alert = format!(
                    "⚠️ *Daily DB backup failed*\n\nError: `{}`\n\nCheck BACKUP_DIR permissions and pg_dump availability.",
                    e
                );
                if let Some(count) = self
                    .state
                    .task_health
                    .record_error("daily_db_backup", &e.to_string())
                    .await
                {
                    // Уведомляем только когда счётчик пересёк порог (не при каждой ошибке)
                    let _ = count;
                    self.state
                        .bot_manager
                        .notify_admins(&self.state.pool, &alert)
                        .await;
                }
            }
        }
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
                        if let Err(e) =
                            sqlx::query("UPDATE nodes SET last_sni_rotation = $1 WHERE id = $2")
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

/// Публичная обёртка над escape_md — используется другими сервисами.
pub fn escape_md_pub(s: &str) -> String {
    escape_md(s)
}

/// Экранирует специальные символы MarkdownV2 для Telegram.
/// Список символов взят из официальной документации Bot API.
fn escape_md(s: &str) -> String {
    let special = [
        '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if special.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Суточное пополнение трафика для подписок с daily_traffic_mb > 0.
///
/// Логика:
/// - Берём все активные подписки, где план имеет daily_traffic_mb > 0.
/// - Если last_daily_topup_at < начала текущего дня (UTC) — делаем пополнение:
///   уменьшаем used_traffic на daily_traffic_mb.
/// - Пол пополнения = 0. Раньше он опускался до -onboarding_bonus_bytes ради
///   одноразового онбординг-запаса; запаса больше нет — трафик приходит только
///   от плана, — и отрицательный used_traffic вместе с ним (поведение для
///   живых юзеров не меняется). У аккаунта с одноразовым онбординг-грантом
///   used_traffic засеян в -bonus, и floor -bonus сохраняет этот headroom —
///   суточное пополнение больше не стирает одноразовый онбординг (см. major-2).
/// - Одновременно сбрасываем last_daily_topup_at = CURRENT_DATE (начало дня UTC).
///
/// Функция вынесена за impl чтобы её можно было вызывать из мониторинга по пулу
/// без копирования AppState.
/// Возвращает plan_id подписок, возвращённых из 'throttled' в 'active' — по
/// ним вызывающая сторона должна разослать регенерацию конфигов нод.
async fn daily_traffic_topup(pool: &PgPool) -> anyhow::Result<Vec<i64>> {
    // 'throttled' — временная блокировка бесплатного плана за исчерпание
    // суточного трафика (см. traffic_service::enforce_quotas): такие подписки
    // обязаны получать пополнение, иначе они не восстановятся никогда.
    let rows = sqlx::query(
        r#"
        UPDATE subscriptions s
        SET used_traffic = GREATEST(
                0,
                s.used_traffic - (p.daily_traffic_mb::BIGINT * 1024 * 1024)
            ),
            last_daily_topup_at = CURRENT_DATE::TIMESTAMPTZ
        FROM plans p
        WHERE s.plan_id = p.id
          AND s.status IN ('active', 'throttled')
          AND COALESCE(p.daily_traffic_mb, 0) > 0
          AND (
              s.last_daily_topup_at IS NULL
              OR s.last_daily_topup_at < CURRENT_DATE::TIMESTAMPTZ
          )
        "#,
    )
    .execute(pool)
    .await?;

    if rows.rows_affected() > 0 {
        info!(
            "Суточное пополнение трафика: обновлено {} подписок",
            rows.rows_affected()
        );
    }

    // Снимаем троттлинг с подписок, чей used_traffic после пополнения снова
    // ниже лимита плана (лимит 0 = безлимит). Возвращаем их plan_id, чтобы
    // ноды перегенерировали конфиги и вернули пользователей в строй.
    // Потолок здесь обязан совпадать с тем, по которому подписку затроттлили
    // (subscription_service::throttle_free_quota_subscriptions), иначе юзер с
    // бонусным трафиком либо застрянет в 'throttled', либо будет флапать.
    let reactivate_sql = format!(
        r#"
        UPDATE subscriptions s
        SET status = 'active'
        FROM plans p, users u
        WHERE s.plan_id = p.id
          AND u.id = s.user_id
          AND s.status = 'throttled'
          AND (
              COALESCE(p.traffic_limit_gb, 0) = 0
              OR COALESCE(s.used_traffic, 0) < {limit}
          )
        RETURNING s.plan_id
        "#,
        limit = crate::services::bonus_traffic::QUOTA_LIMIT_BYTES_SQL,
    );
    let reactivated_plan_ids: Vec<i64> =
        sqlx::query_scalar(&reactivate_sql).fetch_all(pool).await?;

    if !reactivated_plan_ids.is_empty() {
        info!(
            "Суточное пополнение: {} подписок возвращено из 'throttled' в 'active'",
            reactivated_plan_ids.len()
        );
    }

    let mut plan_ids = reactivated_plan_ids;
    plan_ids.sort_unstable();
    plan_ids.dedup();
    Ok(plan_ids)
}

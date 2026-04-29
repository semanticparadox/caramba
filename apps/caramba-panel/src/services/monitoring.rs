use crate::AppState;
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
                Ok(_) => self.state.task_health.record_success("check_node_status").await,
                Err(e) => {
                    error!("Monitoring error (node status): {}", e);
                    self.record_error("check_node_status", &e.to_string()).await;
                }
            }
            match self.check_frontend_status().await {
                Ok(_) => self.state.task_health.record_success("check_frontend_status").await,
                Err(e) => {
                    error!("Monitoring error (frontend status): {}", e);
                    self.record_error("check_frontend_status", &e.to_string()).await;
                }
            }

            if minute_counter % 5 == 0 {
                match self.check_expirations().await {
                    Ok(_) => self.state.task_health.record_success("check_expirations").await,
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
                    Ok(_) => self.state.task_health.record_success("process_auto_renewals").await,
                    Err(e) => {
                        error!("Auto-renewal processing error: {}", e);
                        self.record_error("process_auto_renewals", &e.to_string()).await;
                    }
                }
                match self.check_low_balances().await {
                    Ok(_) => self.state.task_health.record_success("check_low_balances").await,
                    Err(e) => {
                        error!("Low balance check error: {}", e);
                        self.record_error("check_low_balances", &e.to_string()).await;
                    }
                }
            }

            // Каждые 60 тиков (30 минут при тике 30 сек): суточное пополнение трафика.
            // Пополнение делается на daily_traffic_mb МБ — восстанавливает доступ
            // пользователям бесплатного плана, исчерпавшим дневной лимит.
            if minute_counter % 60 == 0 {
                match daily_traffic_topup(&self.state.pool).await {
                    Ok(_) => self.state.task_health.record_success("daily_traffic_topup").await,
                    Err(e) => {
                        error!("Daily traffic top-up error: {}", e);
                        self.record_error("daily_traffic_topup", &e.to_string()).await;
                    }
                }
            }

            if minute_counter % 360 == 0 {
                match self.check_traffic_alerts().await {
                    Ok(_) => self.state.task_health.record_success("check_traffic_alerts").await,
                    Err(e) => {
                        error!("Traffic alerts error: {}", e);
                        self.record_error("check_traffic_alerts", &e.to_string()).await;
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
                        self.state.task_health.record_success("auto_close_stale_tickets").await;
                        if n > 0 {
                            info!("auto_close_stale_tickets: закрыто {} тикетов", n);
                        }
                    }
                    Err(e) => {
                        error!("auto_close_stale_tickets error: {}", e);
                        self.record_error("auto_close_stale_tickets", &e.to_string()).await;
                    }
                }
            }

            if minute_counter % 60 == 0 {
                match self.check_and_rotate_snis().await {
                    Ok(_) => self.state.task_health.record_success("check_and_rotate_snis").await,
                    Err(e) => {
                        error!("Auto SNI Rotation check error: {}", e);
                        self.record_error("check_and_rotate_snis", &e.to_string()).await;
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
            let names: Vec<String> = going_offline.iter().map(|(n, c)| format!("{} ({})", n, c)).collect();
            self.state.bot_manager.notify_admins(
                &self.state.pool,
                &format!("🔴 Nodes went OFFLINE: {}", names.join(", ")),
            ).await;
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
                    // Получаем tg_id и язык пользователя одним запросом
                    let user_row = sqlx::query_as::<_, (i64, Option<String>)>(
                        "SELECT tg_id, language_code FROM users WHERE id = $1",
                    )
                    .bind(user_id)
                    .fetch_optional(&self.state.pool)
                    .await;

                    if let Ok(Some((tg_id, lang))) = user_row {
                        // Получаем новую дату истечения после продления
                        let new_expires: Option<chrono::DateTime<Utc>> =
                            sqlx::query_scalar("SELECT expires_at FROM subscriptions WHERE id = $1")
                                .bind(sub_id)
                                .fetch_optional(&self.state.pool)
                                .await
                                .unwrap_or(None);

                        let expires_str = new_expires
                            .map(|dt| dt.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "N/A".to_string());

                        let lang_ref = lang.as_deref();
                        let amount_str = format!("{:.2}", amount as f64 / 100.0);
                        let is_ru = lang_ref.map_or(true, |l| l.starts_with("ru"));

                        // Экранируем символы для MarkdownV2
                        let plan_escaped = escape_md(&plan_name);
                        let expires_escaped = escape_md(&expires_str);

                        let msg = if is_ru {
                            format!(
                                "✅ *Подписка автоматически продлена*\n\n\
                                 💎 Тариф: *{plan_escaped}*\n\
                                 📅 Действует до: *{expires_escaped}*\n\
                                 💳 Списано: *${amount_str}*"
                            )
                        } else {
                            format!(
                                "✅ *Subscription Auto\\-Renewed*\n\n\
                                 💎 Plan: *{plan_escaped}*\n\
                                 📅 Valid until: *{expires_escaped}*\n\
                                 💳 Charged: *${amount_str}*"
                            )
                        };

                        let _ = self.state.bot_manager.send_notification(tg_id, &msg).await;

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

                        let lang_ref = lang.as_deref();
                        let is_ru = lang_ref.map_or(true, |l| l.starts_with("ru"));
                        let plan_escaped = escape_md(&plan_name);
                        let avail_str = format!("{:.2}", available as f64 / 100.0);
                        let req_str = format!("{:.2}", required as f64 / 100.0);

                        let msg = if is_ru {
                            format!(
                                "⚠️ *Автопродление не выполнено*\n\n\
                                 💎 Тариф: *{plan_escaped}*\n\
                                 💰 Баланс: *${avail_str}*\n\
                                 💳 Требуется: *${req_str}*\n\n\
                                 Пополните баланс, чтобы продолжить пользоваться VPN\\."
                            )
                        } else {
                            format!(
                                "⚠️ *Auto\\-Renewal Failed*\n\n\
                                 💎 Plan: *{plan_escaped}*\n\
                                 💰 Balance: *${avail_str}*\n\
                                 💳 Required: *${req_str}*\n\n\
                                 Please top up your account to keep your VPN access\\."
                            )
                        };

                        let _ = self.state.bot_manager.send_notification(tg_id, &msg).await;

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
            let already_warned = self
                .state
                .redis
                .exists(&redis_key)
                .await
                .unwrap_or(false);

            if already_warned {
                continue;
            }

            // Получаем актуальный баланс для отображения пользователю
            let balance: i64 =
                sqlx::query_scalar("SELECT balance FROM users WHERE id = $1")
                    .bind(user_db_id)
                    .fetch_optional(&self.state.pool)
                    .await
                    .unwrap_or(None)
                    .unwrap_or(0);

            let balance_str = format!("{:.2}", balance as f64 / 100.0);
            let plan_escaped = escape_md(&plan_name);
            let lang_ref = lang.as_deref();
            let is_ru = lang_ref.map_or(true, |l| l.starts_with("ru"));

            let msg = if is_ru {
                format!(
                    "⚠️ *Баланс заканчивается*\n\n\
                     Ваш текущий баланс: *${balance_str}*\n\n\
                     Для автопродления подписки «{plan_escaped}» необходимо пополнить счёт\\. \
                     Пополните баланс заранее, чтобы не потерять доступ\\."
                )
            } else {
                format!(
                    "⚠️ *Balance Running Low*\n\n\
                     Your current balance: *${balance_str}*\n\n\
                     Top up to ensure auto\\-renewal of your «{plan_escaped}» subscription \
                     and avoid losing access\\."
                )
            };

            let _ = self.state.bot_manager.send_notification(tg_id, &msg).await;

            // Ставим флаг в Redis на 24 часа — не беспокоим снова до следующих суток
            if let Err(e) = self.state.redis.set(&redis_key, "1", 86400).await {
                error!("Failed to set balance_warned Redis key for user {}: {}", user_db_id, e);
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
            if let Ok(Some(user)) =
                sqlx::query_as::<_, (i64,)>("SELECT tg_id FROM users WHERE id = $1")
                    .bind(user_id)
                    .fetch_optional(&self.state.pool)
                    .await
            {
                let msg = match alert_type {
                    AlertType::Traffic80 => {
                        "⚠️ *Traffic Warning*\n\n\
                         You've used *80%* of your monthly traffic\\.\n\
                         Consider upgrading your plan to avoid interruption\\."
                    }
                    AlertType::Traffic90 => {
                        "🔶 *Traffic Critical*\n\n\
                         You've used *90%* of your traffic\\.\n\
                         _Access will be paused when the limit is reached\\._"
                    }
                    AlertType::TrafficExceeded => {
                        "🔴 *Traffic Limit Reached*\n\n\
                         Your traffic quota is exhausted\\.\n\
                         Access has been paused\\. Upgrade or wait for the daily top\\-up to resume\\."
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

/// Публичная обёртка над escape_md — используется другими сервисами.
pub fn escape_md_pub(s: &str) -> String {
    escape_md(s)
}

/// Экранирует специальные символы MarkdownV2 для Telegram.
/// Список символов взят из официальной документации Bot API.
fn escape_md(s: &str) -> String {
    let special = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];
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
///   уменьшаем used_traffic на daily_traffic_mb (не уходим в минус).
/// - Одновременно сбрасываем last_daily_topup_at = CURRENT_DATE (начало дня UTC).
///
/// Функция вынесена за impl чтобы её можно было вызывать из мониторинга по пулу
/// без копирования AppState.
async fn daily_traffic_topup(pool: &PgPool) -> anyhow::Result<()> {
    let rows = sqlx::query(
        r#"
        UPDATE subscriptions s
        SET used_traffic = GREATEST(0, s.used_traffic - (p.daily_traffic_mb::BIGINT * 1024 * 1024)),
            last_daily_topup_at = CURRENT_DATE::TIMESTAMPTZ
        FROM plans p
        WHERE s.plan_id = p.id
          AND s.status = 'active'
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

    Ok(())
}

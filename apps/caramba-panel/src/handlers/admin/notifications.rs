// Notifications Module
// Dedicated notifications page, broadcast composer, campaign history, expiry reminders

use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use axum_extra::extract::cookie::CookieJar;
use tracing::error;

use super::auth::get_auth_user;
use super::users::{
    NotificationCampaignHistory, ensure_notification_tables, fetch_campaign_history,
};
use crate::AppState;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "notifications.html")]
pub struct NotificationsTemplate {
    pub campaigns: Vec<NotificationCampaignHistory>,
    pub expiry_reminders_enabled: bool,
    pub expiry_hours_threshold: i64,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_notifications_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if let Err(e) = ensure_notification_tables(&state.pool).await {
        error!("Failed to ensure notification tables: {}", e);
    }
    let campaigns = match fetch_campaign_history(&state.pool).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to fetch notification campaign history: {}", e);
            Vec::new()
        }
    };

    let expiry_reminders_enabled = state
        .settings
        .get_or_default("expiry_reminders_enabled", "false")
        .await
        == "true";
    let expiry_hours_threshold: i64 = state
        .settings
        .get_or_default("expiry_hours_threshold", "72")
        .await
        .parse()
        .unwrap_or(72);

    let template = NotificationsTemplate {
        campaigns,
        expiry_reminders_enabled,
        expiry_hours_threshold,
        is_auth: true,
        username: get_auth_user(&state, &jar)
            .await
            .unwrap_or("Admin".to_string()),
        admin_path: state.admin_path.clone(),
        active_page: "notifications".to_string(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template error: {}", e),
        )
            .into_response(),
    }
}

/// Background task: check for expiring subscriptions and send Telegram reminders.
/// Called from a spawned tokio task on startup.
pub async fn run_expiry_reminder_loop(state: AppState) {
    use tokio::time::{Duration, sleep};
    loop {
        // Wait 6 hours between checks
        sleep(Duration::from_secs(6 * 60 * 60)).await;

        let enabled = state
            .settings
            .get_or_default("expiry_reminders_enabled", "false")
            .await
            == "true";
        if !enabled {
            continue;
        }

        let hours: i64 = state
            .settings
            .get_or_default("expiry_hours_threshold", "72")
            .await
            .parse()
            .unwrap_or(72);

        if let Err(e) = send_expiry_reminders(&state, hours).await {
            error!("Expiry reminder task failed: {}", e);
        }
    }
}

async fn send_expiry_reminders(
    state: &AppState,
    hours_threshold: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Find users with expiring subs who haven't been notified yet
    let rows: Vec<(i64, i64, String)> = sqlx::query_as(
        r#"
        SELECT DISTINCT s.user_id, u.tg_id, s.expires_at::TEXT
        FROM subscriptions s
        JOIN users u ON u.id = s.user_id
        WHERE s.status = 'active'
          AND s.expires_at > NOW()
          AND s.expires_at <= NOW() + ($1::TEXT || ' hours')::INTERVAL
          AND u.tg_id > 0
          AND u.is_banned = FALSE
          AND NOT EXISTS (
              SELECT 1 FROM notification_deliveries nd
              JOIN notification_campaigns nc ON nc.id = nd.campaign_id
              WHERE nd.user_id = s.user_id
                AND nc.target_segment = 'auto_expiry'
                AND nd.sent_at > NOW() - INTERVAL '48 hours'
          )
        "#,
    )
    .bind(hours_threshold.to_string())
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    // Ensure tables exist
    let _ = ensure_notification_tables(&state.pool).await;

    // Create campaign
    let campaign_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO notification_campaigns
            (title, created_by_username, target_segment, parse_mode, media_type, message_text, planned_count, status)
        VALUES
            ('Subscription Expiry Reminder', 'system', 'auto_expiry', 'markdown', 'none', $1, $2, 'running')
        RETURNING id
        "#,
    )
    .bind("⏰ Your subscription expires soon\\. Renew now to keep your connection active\\!".to_string())
    .bind(rows.len().min(i32::MAX as usize) as i32)
    .fetch_one(&state.pool)
    .await?;

    let mut sent = 0i32;
    let mut failed = 0i32;
    for (user_id, tg_id, _expires) in &rows {
        let msg = "⏰ *Subscription Expiring Soon*\\!\n\nYour subscription is about to expire\\. Renew now to keep your connection active\\.";
        match state.bot_manager.send_notification(*tg_id, msg).await {
            Ok(_) => {
                sent += 1;
                let _ = sqlx::query(
                    "INSERT INTO notification_deliveries (campaign_id, user_id, tg_id, status) VALUES ($1, $2, $3, 'sent')",
                )
                .bind(campaign_id)
                .bind(user_id)
                .bind(tg_id)
                .execute(&state.pool)
                .await;
            }
            Err(e) => {
                failed += 1;
                error!("Failed to send expiry reminder to tg_id {}: {}", tg_id, e);
                let _ = sqlx::query(
                    "INSERT INTO notification_deliveries (campaign_id, user_id, tg_id, status, error_text) VALUES ($1, $2, $3, 'failed', $4)",
                )
                .bind(campaign_id)
                .bind(user_id)
                .bind(tg_id)
                .bind(e.to_string())
                .execute(&state.pool)
                .await;
            }
        }
    }

    let status = if sent == 0 && failed > 0 {
        "failed"
    } else if failed > 0 {
        "partial"
    } else {
        "completed"
    };

    let _ = sqlx::query(
        "UPDATE notification_campaigns SET sent_count = $2, failed_count = $3, status = $4 WHERE id = $1",
    )
    .bind(campaign_id)
    .bind(sent)
    .bind(failed)
    .bind(status)
    .execute(&state.pool)
    .await;

    tracing::info!(
        "Expiry reminders: sent={}, failed={}, campaign={}",
        sent,
        failed,
        campaign_id
    );
    Ok(())
}

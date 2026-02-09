// Users Module  
// User management, subscriptions, balance, devices

use axum::{
    extract::{State, Form, Path, Query},
    response::{IntoResponse, Html},
};
use askama::Template;
use askama_web::WebTemplate;
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::{info, error};

use crate::AppState;
use crate::models::store::{User, Plan, Order};
use crate::services::logging_service::LoggingService;
use super::auth::{get_auth_user, is_authenticated};

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "users.html")]
pub struct UsersTemplate {
    pub users: Vec<User>,
    pub search: String,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "user_details.html")]
pub struct UserDetailsTemplate {
    pub user: User,
    pub subscriptions: Vec<SubscriptionWithPlan>,
    pub orders: Vec<UserOrderDisplay>,
    pub referrals: Vec<crate::models::store::DetailedReferral>,
    pub total_referral_earnings: String,
    pub available_plans: Vec<Plan>,
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SubscriptionWithPlan {
    pub id: i64,
    pub plan_name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
    pub price: i64,
    pub active_devices: i64,
    pub device_limit: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UserOrderDisplay {
    pub id: i64,
    pub total_amount: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct AdminGiftForm {
    pub duration_id: i64,
}

#[derive(Deserialize)]
pub struct UpdateUserForm {
    pub balance: i64,
    pub is_banned: bool,
    pub referral_code: Option<String>,
}

#[derive(Deserialize)]
pub struct RefundForm {
    pub amount: i64,
}

#[derive(Deserialize)]
pub struct ExtendForm {
    pub days: i32,
}

// Helper function
fn format_duration(duration: chrono::Duration) -> String {
    if duration.num_seconds() < 60 {
        format!("{} sec", duration.num_seconds())
    } else if duration.num_minutes() < 60 {
        format!("{} min", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{} hr", duration.num_hours())
    } else {
        format!("{} days", duration.num_days())
    }
}

// ============================================================================
// Route Handlers
// ============================================================================

pub async fn get_users(
    State(state): State<AppState>,
    jar: CookieJar,
    query: Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let search = query.get("search").cloned().unwrap_or_default();
    let users = if search.is_empty() {
        state.user_service.get_all().await.unwrap_or_default()
    } else {
        state.user_service.search(&search).await.unwrap_or_default()
    };

    let template = UsersTemplate { users, search, is_auth: true, username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()), admin_path: state.admin_path.clone(), active_page: "users".to_string() };
    
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Template error: {}", e)).into_response(),
    }
}

pub async fn admin_gift_subscription(
    Path(user_id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<AdminGiftForm>,
) -> impl IntoResponse {
    let duration = match state.store_service.get_plan_duration_by_id(form.duration_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return (axum::http::StatusCode::BAD_REQUEST, "Invalid duration ID").into_response(),
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response(),
    };

    match state.subscription_service.admin_gift_subscription(user_id, duration.plan_id, duration.duration_days).await {
        Ok(sub) => {
            if let Ok(Some(user)) = state.user_service.get_by_id(user_id).await {
                 let msg = format!("🎁 *Gift Received\\!*\\n\\nYou have received a new subscription\\.\\nExpires: {}", sub.expires_at.format("%Y-%m-%d"));
                 let _ = state.bot_manager.send_notification(
                     user.tg_id,
                     &msg
                 ).await;
            }

            let admin_path = state.admin_path.clone();
            return axum::response::Redirect::to(&format!("{}/users/{}", admin_path, user_id)).into_response();
        },
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR, 
                format!("Failed to gift subscription: {}", e)
            ).into_response();
        }
    }
}

pub async fn get_user_details(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    let user = state.user_service.get_by_id(id).await.unwrap_or(None);

    let user = match user {
        Some(u) => u,
        None => return (axum::http::StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let subscriptions = match state.subscription_service.get_subscriptions_with_details_for_admin(id).await {
        Ok(subs) => subs,
        Err(e) => {
            error!("Failed to fetch user subscriptions: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to fetch subs: {}", e)).into_response();
        }
    };

    let db_orders = state.billing_service.get_user_orders(id).await.map_err(|e| {
         error!("Failed to fetch user orders: {}", e);
         e
    }).unwrap_or_default();

    let orders = db_orders.into_iter().map(|o| UserOrderDisplay {
        id: o.id,
        total_amount: format!("{:.2}", o.total_amount as f64 / 100.0),
        status: o.status,
        created_at: o.created_at.format("%Y-%m-%d").to_string(),
    }).collect();

    let referrals = state.store_service.get_user_referrals(id).await.unwrap_or_default();
    let earnings_cents = state.store_service.get_user_referral_earnings(id).await.unwrap_or(0);

    let available_plans = state.store_service.get_active_plans().await.unwrap_or_default();

    let template = UserDetailsTemplate {
        user,
        subscriptions,
        orders,
        referrals,
        total_referral_earnings: format!("{:.2}", earnings_cents as f64 / 100.0),
        available_plans,
        is_auth: true,
        username: get_auth_user(&state, &jar).await.unwrap_or("Admin".to_string()),
        admin_path: state.admin_path.clone(),
        active_page: "users".to_string(),
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

pub async fn update_user(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<UpdateUserForm>,
) -> impl IntoResponse {
    let old_user = state.user_service.get_by_id(id).await.unwrap_or(None);

    let res = state.user_service.update_profile(id, form.balance, form.is_banned, form.referral_code.as_deref().map(|s| s.trim())).await;

    match res {
        Ok(_) => {
            let _ = crate::services::activity_service::ActivityService::log(&state.pool, "User", &format!("User {} updated: Balance={}, Banned={}", id, form.balance, form.is_banned)).await;
            
            if let Some(u) = old_user {
                if u.is_banned != form.is_banned {
                    let msg = if form.is_banned {
                        "🚫 *Account Banned*\\n\\nYour account has been suspended by an administrator\\."
                    } else {
                        "✅ *Account Unbanned*\\n\\nYour account has been reactivated\\. Welcome back\\!"
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, msg).await;
                }

                if u.balance != form.balance {
                    let diff = form.balance - u.balance;
                    let amount = format!("{:.2}", diff.abs() as f64 / 100.0);
                    let msg = if diff > 0 {
                        format!("💰 *Balance Updated*\\n\\nAdministrator added *${}* to your account\\.", amount)
                    } else {
                        format!("📉 *Balance Updated*\\n\\nAdministrator deducted *${}* from your account\\.", amount)
                    };
                    let _ = state.bot_manager.send_notification(u.tg_id, &msg).await;
                }
            }

            let admin_path = state.admin_path.clone();
            ([(("HX-Redirect", format!("{}/users/{}", admin_path, id)))], "Updated").into_response()
        },
        Err(e) => {
            error!("Failed to update user {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update user").into_response()
        }
    }
}

pub async fn update_user_balance(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let balance_str = form.get("balance").unwrap_or(&"0".to_string()).clone();
    let balance: i64 = balance_str.parse().unwrap_or(0);

    let res = state.user_service.set_balance(id, balance).await;

    match res {
        Ok(_) => {
            let _ = LoggingService::log_system(
                &state.pool,
                "admin_update_balance",
                &format!("Admin updated user {} balance to {} cents", id, balance)
            ).await;
            
            let admin_path = state.admin_path.clone();
            ([(("HX-Redirect", format!("{}/users", admin_path)))], "Updated").into_response()
        },
        Err(e) => {
            error!("Failed to update balance for user {}: {}", id, e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update balance").into_response()
        }
    }
}

pub async fn delete_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("Request to delete subscription ID: {}", id);
    match state.subscription_service.admin_delete(id).await {
        Ok(_) => (axum::http::StatusCode::OK, "").into_response(),
        Err(e) => {
             error!("Failed to delete subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)).into_response()
        }
    }
}

pub async fn refund_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<RefundForm>,
) -> impl IntoResponse {
    info!("Request to refund subscription ID: {} with amount {}", id, form.amount);
    match state.store_service.admin_refund_subscription(id, form.amount).await {
        Ok(_) => ([(("HX-Refresh", "true"))], "Refunded").into_response(),
        Err(e) => {
             error!("Failed to refund subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to refund: {}", e)).into_response()
        }
    }
}

pub async fn extend_user_subscription(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Form(form): Form<ExtendForm>,
) -> impl IntoResponse {
    info!("Request to extend subscription ID: {} by {} days", id, form.days);
    match state.subscription_service.admin_extend(id, form.days).await {
        Ok(_) => ([(("HX-Refresh", "true"))], "Extended").into_response(),
        Err(e) => {
             error!("Failed to extend subscripton {}: {}", id, e);
             (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to extend: {}", e)).into_response()
        }
    }
}

pub async fn get_subscription_devices(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let ips = match state.subscription_service.get_active_ips(sub_id).await {
        Ok(ips) => ips,
        Err(e) => {
            error!("Failed to fetch IPs for sub {}: {}", sub_id, e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch devices").into_response();
        }
    };

    let admin_path = state.admin_path.clone();

    let mut html = String::new();
    
    html.push_str(&format!(
        r##"
        <div class="flex justify-between items-center mb-6 p-4 rounded-2xl bg-orange-500/10 border border-orange-500/10 shadow-lg shadow-orange-500/5">
            <div>
                <p class="text-sm font-bold text-orange-400 mb-0.5">Manage Active Sessions</p>
                <p class="text-[11px] text-slate-500">Disconnect all current devices immediately</p>
            </div>
            <button hx-post="{}/subs/{}/devices/kill" hx-target="#devices_content" hx-confirm="This will disconnect ALL currently connected users for this subscription. Continue?"
                class="px-4 py-2 rounded-xl bg-orange-600 hover:bg-orange-500 text-white text-xs font-bold transition-all shadow-lg shadow-orange-500/20 active:scale-95">
                Reset All
            </button>
        </div>
        "##, admin_path, sub_id
    ));

    if ips.is_empty() {
        html.push_str("<div class='py-12 text-center text-slate-500 border border-white/5 rounded-2xl bg-slate-950/20'><p class='text-sm'>No active devices detected in the last 15 minutes.</p></div>");
        return Html(html).into_response();
    }

    html.push_str("<div class='overflow-hidden rounded-2xl border border-white/5 bg-slate-950/30 shadow-inner'>");
    html.push_str("<table class='w-full text-left border-collapse'>");
    html.push_str("<thead><tr class='text-[10px] font-bold text-slate-500 uppercase tracking-widest bg-white/5'><th class='px-6 py-3'>Client IP Address</th><th class='px-6 py-3'>Activity</th></tr></thead>");
    html.push_str("<tbody class='divide-y divide-white/5'>");
    for ip_record in ips {
        let time_ago = format_duration(chrono::Utc::now() - ip_record.last_seen_at);
        html.push_str(&format!(
            "<tr class='hover:bg-white/5 transition-colors'><td class='px-6 py-4'><div class='flex items-center gap-2'><i data-lucide='globe' class='w-3 h-3 text-indigo-500 opacity-50'></i><code class='text-sm text-indigo-400 font-mono'>{}</code></div></td><td class='px-6 py-4 text-xs text-slate-400 font-medium'>{} ago</td></tr>",
            ip_record.client_ip, time_ago
        ));
    }
    html.push_str("</tbody></table></div>");

    Html(html).into_response()
}

pub async fn admin_kill_subscription_sessions(
    State(state): State<AppState>,
    Path(sub_id): Path<i64>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let sub = match state.subscription_service.get_by_id(sub_id).await {
        Ok(Some(s)) => s,
        _ => return (axum::http::StatusCode::NOT_FOUND, "Subscription not found").into_response(),
    };

    let uuid = match sub.vless_uuid {
        Some(u) => u,
        None => return (axum::http::StatusCode::BAD_REQUEST, "No UUID for sub").into_response(),
    };

    if let Err(e) = state.connection_service.kill_subscription_connections(&uuid).await {
        error!("Admin failed to kill sessions for sub {}: {}", sub_id, e);
        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to kill sessions: {}", e)).into_response();
    }

    let success_html = format!(
        r##"
        <div class="flex flex-col items-center justify-center py-12 text-center animate-fade-in">
            <div class="w-20 h-20 rounded-3xl bg-emerald-500/10 flex items-center justify-center mb-6 text-emerald-400 border border-emerald-500/20 shadow-xl shadow-emerald-500/10 transform rotate-3">
                <i data-lucide='check-circle' class="w-10 h-10"></i>
            </div>
            <h4 class="text-xl font-bold text-white mb-2 tracking-tight">Sessions Reset Successfully</h4>
            <p class="text-sm text-slate-500 mb-8 px-12 leading-relaxed">All active connections for subscription #{} have been terminated. It may take up to 60 seconds for all caches to clear.</p>
            <button hx-get="{}/subs/{}/devices" hx-target="#devices_content"
                class="px-5 py-2.5 rounded-xl bg-white/10 hover:bg-white/20 border border-white/10 text-white text-sm font-bold transition-all active:scale-95" style="backdrop-filter: blur(10px);">
                Refresh Device List
            </button>
        </div>
        "##, sub_id, state.admin_path, sub_id
    );

    Html(success_html).into_response()
}

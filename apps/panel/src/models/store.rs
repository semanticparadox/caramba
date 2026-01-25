use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: i64,
    pub tg_id: i64,
    pub username: Option<String>,
    pub full_name: Option<String>,
    pub balance: i64,
    pub referral_code: Option<String>,
    pub referrer_id: Option<i64>,
    pub is_banned: bool,
    pub language_code: Option<String>,
    pub terms_accepted_at: Option<DateTime<Utc>>,
    pub warning_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Plan {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub traffic_limit_gb: i32,
    pub device_limit: i32,
    pub created_at: DateTime<Utc>,
    #[sqlx(skip)]
    pub durations: Vec<PlanDuration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PlanDuration {
    pub id: i64,
    pub plan_id: i64,
    pub duration_days: i32,
    pub price: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Subscription {
    pub id: i64,
    pub user_id: i64,
    pub plan_id: i64,
    pub node_id: Option<i64>,
    pub vless_uuid: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub status: String,
    pub used_traffic: i64,
    pub traffic_updated_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PromoCode {
    pub id: i64,
    pub code: String,
    pub discount_percent: i32,
    pub bonus_amount: i64,
    pub max_uses: i32,
    pub current_uses: i32,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GiftCode {
    pub id: i64,
    pub code: String,
    pub plan_id: i64,
    pub duration_days: i32,
    pub created_by: i64,
    pub redeemed_by: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub redeemed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(dead_code)]
pub struct Payment {
    pub id: i64,
    pub user_id: i64,
    pub method: String,
    pub amount: i64,
    pub external_id: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Product {
    pub id: i64,
    pub category_id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub price: i64,
    pub product_type: String, // 'file', 'text', 'subscription'
    pub content: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Order {
    pub id: i64,
    pub user_id: i64,
    pub total_amount: i64,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[allow(dead_code)]
pub struct OrderItem {
    pub id: i64,
    pub order_id: i64,
    pub product_id: i64,
    pub price_at_purchase: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SubscriptionIpTracking {
    pub id: i64,
    pub subscription_id: i64,
    pub client_ip: String,
    pub last_seen_at: DateTime<Utc>,
}

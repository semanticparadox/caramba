use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PromoCode {
    pub id: i64,
    pub code: String,
    #[sqlx(rename = "type")]
    pub promo_type: String, // sqlx will map 'type' column to this
    pub plan_id: Option<i64>,
    pub balance_amount: Option<i32>,
    pub duration_days: Option<i32>,
    pub traffic_gb: Option<i32>,
    pub max_uses: i32,
    pub current_uses: i32,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub created_by_admin_id: Option<i64>,
    pub promoter_user_id: Option<i64>,
    pub is_active: bool,
}

impl PromoCode {
    pub fn usage_pct(&self) -> f32 {
        if self.max_uses == 0 {
            return 0.0;
        }
        (self.current_uses as f32 / self.max_uses as f32) * 100.0
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PromoCodeUsage {
    pub id: i64,
    pub promo_code_id: i64,
    pub user_id: i64,
    pub used_at: DateTime<Utc>,
}

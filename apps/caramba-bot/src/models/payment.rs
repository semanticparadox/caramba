use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentType {
    BalanceTopup,
    PlanPurchase(i64),    // duration_id
    ProductPurchase(i64), // product_id
}

impl PaymentType {
    pub fn to_payload_string(&self, user_id: i64) -> String {
        match self {
            PaymentType::BalanceTopup => format!("topup:{}", user_id),
            PaymentType::PlanPurchase(id) => format!("plan:{}:{}", user_id, id),
            PaymentType::ProductPurchase(id) => format!("prod:{}:{}", user_id, id),
        }
    }
}

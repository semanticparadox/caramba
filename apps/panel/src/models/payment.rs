use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum PaymentType {
    BalanceTopup,
    OrderPurchase(i64), // order_id
    SubscriptionPurchase(i64), // plan_id
}

impl PaymentType {
    pub fn to_payload_string(&self, user_id: i64) -> String {
        match self {
            PaymentType::BalanceTopup => format!("{}:bal:0", user_id),
            PaymentType::OrderPurchase(order_id) => format!("{}:ord:{}", user_id, order_id),
            PaymentType::SubscriptionPurchase(plan_id) => format!("{}:sub:{}", user_id, plan_id),
        }
    }
}

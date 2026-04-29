use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentType {
    BalanceTopup,
    PlanPurchase(i64),    // duration_id
    ProductPurchase(i64), // product_id
}

/// Результат разбора payload из Telegram-инвойса
#[derive(Debug)]
pub struct ParsedPayload {
    pub user_id: i64,
    pub payment_type: PaymentType,
}

impl PaymentType {
    pub fn to_payload_string(&self, user_id: i64) -> String {
        match self {
            PaymentType::BalanceTopup => format!("topup:{}", user_id),
            PaymentType::PlanPurchase(id) => format!("plan:{}:{}", user_id, id),
            PaymentType::ProductPurchase(id) => format!("prod:{}:{}", user_id, id),
        }
    }

    /// Парсит payload вида "topup:123", "plan:123:456", "prod:123:789"
    /// Возвращает None если формат неизвестен или user_id невалидный
    pub fn from_payload(payload: &str) -> Option<ParsedPayload> {
        let parts: Vec<&str> = payload.splitn(3, ':').collect();
        match parts.as_slice() {
            ["topup", uid] => {
                let user_id = uid.parse::<i64>().ok()?;
                Some(ParsedPayload {
                    user_id,
                    payment_type: PaymentType::BalanceTopup,
                })
            }
            ["plan", uid, plan_id] => {
                let user_id = uid.parse::<i64>().ok()?;
                let id = plan_id.parse::<i64>().ok()?;
                Some(ParsedPayload {
                    user_id,
                    payment_type: PaymentType::PlanPurchase(id),
                })
            }
            ["prod", uid, prod_id] => {
                let user_id = uid.parse::<i64>().ok()?;
                let id = prod_id.parse::<i64>().ok()?;
                Some(ParsedPayload {
                    user_id,
                    payment_type: PaymentType::ProductPurchase(id),
                })
            }
            _ => None,
        }
    }
}

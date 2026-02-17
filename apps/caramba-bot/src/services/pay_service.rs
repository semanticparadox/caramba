use crate::api_client::ApiClient;
use crate::models::payment::PaymentType;
use anyhow::Result;

#[derive(Clone)]
pub struct PayService {
    api: ApiClient,
}

impl PayService {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }

    pub async fn process_any_payment(&self, amount_usd: f64, method: &str, external_id: Option<String>, payload: &str) -> Result<()> {
        #[derive(serde::Serialize)]
        struct PaymentReq<'a> {
            amount_usd: f64,
            method: &'a str,
            external_id: Option<String>,
            payload: &'a str
        }
        let _: serde_json::Value = self.api.post("/payments/process", &PaymentReq { amount_usd, method, external_id, payload }).await?;
        Ok(())
    }

    pub async fn create_cryptobot_invoice(&self, user_id: i64, amount: f64, payment_type: PaymentType) -> Result<String> {
        #[derive(serde::Serialize)]
        struct InvoiceReq { amount: f64, payment_type: PaymentType }
        #[derive(serde::Deserialize)]
        struct InvoiceResp { url: String }
        let resp: InvoiceResp = self.api.post(&format!("/payments/{}/cryptobot", user_id), &InvoiceReq { amount, payment_type }).await?;
        Ok(resp.url)
    }

    pub async fn create_nowpayments_invoice(&self, user_id: i64, amount: f64, payment_type: PaymentType) -> Result<String> {
        #[derive(serde::Serialize)]
        struct InvoiceReq { amount: f64, payment_type: PaymentType }
        #[derive(serde::Deserialize)]
        struct InvoiceResp { url: String }
        let resp: InvoiceResp = self.api.post(&format!("/payments/{}/nowpayments", user_id), &InvoiceReq { amount, payment_type }).await?;
        Ok(resp.url)
    }

    pub async fn create_crystalpay_invoice(&self, user_id: i64, amount: f64, payment_type: PaymentType) -> Result<String> {
        #[derive(serde::Serialize)]
        struct InvoiceReq { amount: f64, payment_type: PaymentType }
        #[derive(serde::Deserialize)]
        struct InvoiceResp { url: String }
        let resp: InvoiceResp = self.api.post(&format!("/payments/{}/crystalpay", user_id), &InvoiceReq { amount, payment_type }).await?;
        Ok(resp.url)
    }

    pub async fn create_stripe_session(&self, user_id: i64, amount: f64, payment_type: PaymentType) -> Result<String> {
        #[derive(serde::Serialize)]
        struct InvoiceReq { amount: f64, payment_type: PaymentType }
        #[derive(serde::Deserialize)]
        struct InvoiceResp { url: String }
        let resp: InvoiceResp = self.api.post(&format!("/payments/{}/stripe", user_id), &InvoiceReq { amount, payment_type }).await?;
        Ok(resp.url)
    }
}

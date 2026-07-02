use crate::api_client::ApiClient;
use anyhow::Result;

#[derive(Clone)]
pub struct PromoService {
    api: ApiClient,
}

impl PromoService {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }

    pub async fn redeem_code(&self, user_id: i64, code: &str) -> Result<String> {
        #[derive(serde::Serialize)]
        struct RedeemReq<'a> {
            code: &'a str,
        }
        #[derive(serde::Deserialize)]
        struct RedeemResp {
            message: String,
        }
        let resp: RedeemResp = self
            .api
            .post(&format!("/users/{}/redeem", user_id), &RedeemReq { code })
            .await?;
        Ok(resp.message)
    }

    pub async fn list_promos(&self) -> Result<Vec<crate::models::store::PromoInfo>> {
        self.api.get("/admin/promos").await
    }

    pub async fn create_promo(&self, code: &str, promo_type: &str, value: i64) -> Result<()> {
        #[derive(serde::Serialize)]
        struct CreateReq {
            code: String,
            promo_type: String,
            value: i64,
        }
        let _: serde_json::Value = self
            .api
            .post(
                "/admin/promos",
                &CreateReq {
                    code: code.to_string(),
                    promo_type: promo_type.to_string(),
                    value,
                },
            )
            .await?;
        Ok(())
    }
}

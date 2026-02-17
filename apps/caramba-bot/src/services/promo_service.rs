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
            code: &'a str
        }
        #[derive(serde::Deserialize)]
        struct RedeemResp {
            message: String
        }
        let resp: RedeemResp = self.api.post(&format!("/users/{}/redeem", user_id), &RedeemReq { code }).await?;
        Ok(resp.message)
    }
}

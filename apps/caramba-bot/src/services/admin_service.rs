use crate::api_client::ApiClient;
use crate::models::store::{SniRotationLog, SniPool};
use anyhow::Result;

#[derive(Clone)]
pub struct AdminService {
    api: ApiClient,
}

impl AdminService {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }

    pub async fn is_admin(&self, tg_id: i64) -> bool {
        #[derive(serde::Serialize)]
        struct AdminCheckReq {
            tg_id: i64
        }
        #[derive(serde::Deserialize)]
        struct AdminCheckResp {
            is_admin: bool
        }
        
        match self.api.post::<AdminCheckResp, _>("/admin/check", &AdminCheckReq { tg_id }).await {
            Ok(resp) => resp.is_admin,
            Err(_) => false,
        }
    }

    pub async fn get_sni_logs(&self) -> Result<Vec<SniRotationLog>> {
        self.api.get("/admin/sni/logs").await
    }

    pub async fn get_sni_pool(&self) -> Result<Vec<SniPool>> {
        self.api.get("/admin/sni/pool").await
    }
}

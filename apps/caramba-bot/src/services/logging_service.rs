use crate::api_client::ApiClient;
use anyhow::Result;

#[derive(Clone)]
pub struct LoggingService {
    api: ApiClient,
}

impl LoggingService {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }

    pub async fn log_user(
        &self,
        tg_id: Option<i64>,
        action: &str,
        details: &str,
        ip: Option<&str>,
    ) -> Result<()> {
        #[derive(serde::Serialize)]
        struct LogReq<'a> {
            tg_id: Option<i64>,
            action: &'a str,
            details: &'a str,
            ip: Option<&'a str>,
        }
        let _: serde_json::Value = self
            .api
            .post(
                "/logs/user",
                &LogReq {
                    tg_id,
                    action,
                    details,
                    ip,
                },
            )
            .await?;
        Ok(())
    }
}

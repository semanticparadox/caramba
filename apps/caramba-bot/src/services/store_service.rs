use crate::api_client::ApiClient;
use crate::models::store::{
    CartItem, DetailedSubscription, GiftCode, Plan, Product, StoreCategory, Subscription,
    SubscriptionIpTracking, User,
};
use anyhow::Result;

#[derive(Clone)]
pub struct StoreService {
    api: ApiClient,
}

impl StoreService {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }

    pub async fn get_user_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        self.api
            .get::<Option<User>>(&format!("/users/tg/{}", tg_id))
            .await
    }

    pub async fn resolve_referrer_id(&self, code: &str) -> Result<Option<i64>> {
        self.api
            .get::<Option<i64>>(&format!("/referrers/resolve/{}", code))
            .await
    }

    pub async fn upsert_user(
        &self,
        tg_id: i64,
        username: Option<&str>,
        full_name: Option<&str>,
        referrer_id: Option<i64>,
    ) -> Result<Option<User>> {
        #[derive(serde::Serialize)]
        struct UpsertReq<'a> {
            tg_id: i64,
            username: Option<&'a str>,
            full_name: Option<&'a str>,
            referrer_id: Option<i64>,
        }
        let req = UpsertReq {
            tg_id,
            username,
            full_name,
            referrer_id,
        };
        self.api.post::<Option<User>, _>("/users", &req).await
    }

    pub async fn get_user_subscriptions(&self, user_id: i64) -> Result<Vec<DetailedSubscription>> {
        self.api
            .get::<Vec<DetailedSubscription>>(&format!("/users/{}/subs", user_id))
            .await
    }

    pub async fn get_active_plans(&self) -> Result<Vec<Plan>> {
        self.api.get::<Vec<Plan>>("/plans").await
    }

    pub async fn get_user_cart(&self, user_id: i64) -> Result<Vec<CartItem>> {
        self.api
            .get::<Vec<CartItem>>(&format!("/users/{}/cart", user_id))
            .await
    }

    pub async fn get_referral_count(&self, user_id: i64) -> Result<i64> {
        self.api
            .get::<i64>(&format!("/users/{}/referrals/count", user_id))
            .await
    }

    pub async fn get_user_referral_earnings(&self, user_id: i64) -> Result<i64> {
        self.api
            .get::<i64>(&format!("/users/{}/referrals/earnings", user_id))
            .await
    }

    pub async fn update_user_referral_code(&self, user_id: i64, code: &str) -> Result<()> {
        #[derive(serde::Serialize)]
        struct UpdateRefCodeReq<'a> {
            code: &'a str,
        }
        let _: serde_json::Value = self
            .api
            .post(
                &format!("/users/{}/referral-code", user_id),
                &UpdateRefCodeReq { code },
            )
            .await?;
        Ok(())
    }

    pub async fn set_user_referrer(&self, user_id: i64, code: &str) -> Result<()> {
        #[derive(serde::Serialize)]
        struct SetRefReq<'a> {
            code: &'a str,
        }
        let _: serde_json::Value = self
            .api
            .post(&format!("/users/{}/referrer", user_id), &SetRefReq { code })
            .await?;
        Ok(())
    }

    pub async fn transfer_subscription(
        &self,
        sub_id: i64,
        user_id: i64,
        target_info: &str,
    ) -> Result<()> {
        #[derive(serde::Serialize)]
        struct TransferReq<'a> {
            user_id: i64,
            target_info: &'a str,
        }
        let _: serde_json::Value = self
            .api
            .post(
                &format!("/subs/{}/transfer", sub_id),
                &TransferReq {
                    user_id,
                    target_info,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn update_subscription_note(&self, sub_id: i64, note: String) -> Result<()> {
        #[derive(serde::Serialize)]
        struct NoteReq {
            note: String,
        }
        let _: serde_json::Value = self
            .api
            .post(&format!("/subs/{}/note", sub_id), &NoteReq { note })
            .await?;
        Ok(())
    }

    pub async fn get_categories(&self) -> Result<Vec<StoreCategory>> {
        self.api
            .get::<Vec<StoreCategory>>("/store/categories")
            .await
    }

    pub async fn get_products_by_category(&self, category_id: i64) -> Result<Vec<Product>> {
        self.api
            .get::<Vec<Product>>(&format!("/store/categories/{}/products", category_id))
            .await
    }

    pub async fn increment_warning_count(&self, user_id: i64) -> Result<()> {
        let _: serde_json::Value = self
            .api
            .post(&format!("/users/{}/warn", user_id), &())
            .await?;
        Ok(())
    }

    pub async fn ban_user(&self, user_id: i64) -> Result<()> {
        let _: serde_json::Value = self
            .api
            .post(&format!("/users/{}/ban", user_id), &())
            .await?;
        Ok(())
    }

    pub async fn get_subscription_active_ips(
        &self,
        sub_id: i64,
    ) -> Result<Vec<SubscriptionIpTracking>> {
        self.api
            .get::<Vec<SubscriptionIpTracking>>(&format!("/subs/{}/ips", sub_id))
            .await
    }

    pub async fn get_subscription_device_limit(&self, sub_id: i64) -> Result<i64> {
        self.api
            .get::<i64>(&format!("/subs/{}/limit", sub_id))
            .await
    }

    pub async fn add_bot_message_to_history(
        &self,
        user_id: i64,
        chat_id: i64,
        message_id: i32,
    ) -> Result<()> {
        #[derive(serde::Serialize)]
        struct AddMsgReq {
            chat_id: i64,
            message_id: i32,
        }
        let _: serde_json::Value = self
            .api
            .post(
                &format!("/users/{}/bot-history", user_id),
                &AddMsgReq {
                    chat_id,
                    message_id,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn cleanup_bot_history(
        &self,
        user_id: i64,
        keep_limit: i32,
    ) -> Result<Vec<(i64, i32)>> {
        #[derive(serde::Serialize)]
        struct CleanupReq {
            keep_limit: i32,
        }
        #[derive(serde::Deserialize)]
        struct CleanupResp {
            deleted_messages: Vec<(i64, i32)>,
        }
        let resp: CleanupResp = self
            .api
            .post(
                &format!("/users/{}/bot-history/cleanup", user_id),
                &CleanupReq { keep_limit },
            )
            .await?;
        Ok(resp.deleted_messages)
    }

    pub async fn update_user_language(&self, user_id: i64, lang: &str) -> Result<()> {
        #[derive(serde::Serialize)]
        struct LangReq<'a> {
            lang: &'a str,
        }
        let _: serde_json::Value = self
            .api
            .post(&format!("/users/{}/language", user_id), &LangReq { lang })
            .await?;
        Ok(())
    }

    pub async fn update_user_terms(&self, user_id: i64) -> Result<()> {
        let _: serde_json::Value = self
            .api
            .post(&format!("/users/{}/terms", user_id), &())
            .await?;
        Ok(())
    }

    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i32) -> Result<()> {
        #[derive(serde::Serialize)]
        struct MsgIdReq {
            msg_id: i32,
        }
        let _: serde_json::Value = self
            .api
            .post(
                &format!("/users/{}/last-msg", user_id),
                &MsgIdReq { msg_id },
            )
            .await?;
        Ok(())
    }

    pub async fn generate_subscription_file(&self, user_id: i64) -> Result<String> {
        let resp: serde_json::Value = self
            .api
            .get(&format!("/users/{}/config-file", user_id))
            .await?;
        Ok(resp.to_string())
    }

    pub async fn activate_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        self.api
            .post::<Subscription, _>(
                &format!("/subs/{}/activate", sub_id),
                &serde_json::json!({ "user_id": user_id }),
            )
            .await
    }

    pub async fn get_user_gift_codes(&self, user_id: i64) -> Result<Vec<GiftCode>> {
        self.api
            .get::<Vec<GiftCode>>(&format!("/users/{}/gifts", user_id))
            .await
    }

    pub async fn convert_subscription_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct GiftResp {
            code: String,
        }
        let resp: GiftResp = self
            .api
            .post(
                &format!("/subs/{}/convert-gift", sub_id),
                &serde_json::json!({ "user_id": user_id }),
            )
            .await?;
        Ok(resp.code)
    }

    pub async fn purchase_plan(&self, user_id: i64, duration_id: i64) -> Result<Subscription> {
        #[derive(serde::Serialize)]
        struct BuyReq {
            duration_id: i64,
        }
        self.api
            .post::<Subscription, _>(
                &format!("/users/{}/purchase-plan", user_id),
                &BuyReq { duration_id },
            )
            .await
    }

    pub async fn extend_subscription(
        &self,
        user_id: i64,
        duration_id: i64,
    ) -> Result<Subscription> {
        #[derive(serde::Serialize)]
        struct ExtReq {
            duration_id: i64,
        }
        self.api
            .post::<Subscription, _>(
                &format!("/users/{}/extend-sub", user_id),
                &ExtReq { duration_id },
            )
            .await
    }

    pub async fn purchase_product_with_balance(
        &self,
        user_id: i64,
        product_id: i64,
    ) -> Result<Product> {
        #[derive(serde::Serialize)]
        struct BuyProdReq {
            product_id: i64,
        }
        self.api
            .post::<Product, _>(
                &format!("/users/{}/purchase-product", user_id),
                &BuyProdReq { product_id },
            )
            .await
    }

    pub async fn get_subscription_links(&self, sub_id: i64) -> Result<Vec<String>> {
        self.api
            .get::<Vec<String>>(&format!("/subs/{}/links", sub_id))
            .await
    }
}

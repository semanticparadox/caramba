use anyhow::Result;
use async_trait::async_trait;
use caramba_db::models::store::{PaymentSession, User};

use super::provider::{PaymentProvider, PaymentWebhookAction};

// StarsProvider не используется в production — см. комментарии в marketplace_service.rs.
// Оставлен как задокументированная заглушка для будущей интеграции.
#[allow(dead_code)]
pub struct StarsProvider;

#[async_trait]
impl PaymentProvider for StarsProvider {
    fn name(&self) -> &str {
        "stars"
    }

    async fn create_invoice(
        &self,
        _session: &PaymentSession,
        _user: &User,
        _client: &reqwest::Client,
    ) -> Result<String> {
        // AMBIGUOUS: Telegram Stars инвойсы создаются через Bot API (createInvoiceLink),
        // который требует bot_token и tg_id пользователя. Эти данные недоступны через
        // текущий интерфейс PaymentProvider.
        //
        // Реальный Stars-флоу (pay_service::create_stars_invoice) работает корректно —
        // он реализован напрямую в PayService с доступом к bot_token.
        //
        // Этот провайдер используется в MarketplaceService и пока не имеет полной реализации.
        // Для полноценной работы нужно либо передавать bot_token в StarsProvider,
        // либо убрать StarsProvider из MarketplaceService и обрабатывать Stars отдельно.
        anyhow::bail!(
            "StarsProvider in MarketplaceService is not implemented. \
             Use PayService::create_stars_invoice for Telegram Stars payments."
        )
    }

    async fn verify_webhook(&self, _payload: &[u8], _signature: &str) -> Result<bool> {
        // Telegram stars are verified via the bot API pre_checkout_query, not a standard webhook
        Ok(true)
    }

    async fn handle_webhook(&self, _payload: &[u8]) -> Result<PaymentWebhookAction> {
        Ok(PaymentWebhookAction::Ignored)
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        Ok("pending".to_string())
    }
}

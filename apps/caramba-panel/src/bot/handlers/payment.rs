use teloxide::prelude::*;
use teloxide::types::PreCheckoutQuery;

pub async fn pre_checkout_handler(
    bot: Bot,
    q: PreCheckoutQuery,
) -> Result<(), teloxide::RequestError> {
    // Determine if we should accept. For now, accept everything.
    bot.answer_pre_checkout_query(q.id, true).await?;
    Ok(())
}

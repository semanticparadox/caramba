use crate::services::logging_service::LoggingService;
use crate::services::pay_service::PayService;
use crate::services::promo_service::PromoService;
use crate::services::settings_service::SettingsService;
use crate::services::store_service::StoreService;

#[derive(Clone)]
pub struct AppState {
    pub settings: SettingsService,
    pub store_service: StoreService,
    pub promo_service: PromoService,
    pub pay_service: PayService,
    pub logging_service: LoggingService,
}

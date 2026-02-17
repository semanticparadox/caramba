use crate::api_client::ApiClient;
use crate::services::store_service::StoreService;
use crate::services::promo_service::PromoService;
use crate::services::pay_service::PayService;
use crate::services::settings_service::SettingsService;
use crate::services::admin_service::AdminService;
use crate::services::logging_service::LoggingService;

#[derive(Clone)]
pub struct AppState {
    pub api: ApiClient,
    pub settings: SettingsService,
    pub store_service: StoreService,
    pub promo_service: PromoService,
    pub pay_service: PayService,
    pub admin_service: AdminService,
    pub logging_service: LoggingService,
}

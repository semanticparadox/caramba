use crate::api_client::ApiClient;
use crate::bot::handlers::admin::AdminFsmStorage;
use crate::services::admin_service::AdminService;
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
    /// Клиент панели для вызова API (signup-бонусы, обновления и т.д.)
    pub api_client: ApiClient,
    /// Сервис административных операций (тикеты, broadcast, проверка прав)
    pub admin_service: AdminService,
    /// In-memory FSM-хранилище состояний администратора (reply/broadcast)
    pub admin_fsm: AdminFsmStorage,
}

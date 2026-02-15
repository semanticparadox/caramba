pub mod store_service;
pub mod orchestration_service;
pub mod pay_service;
pub mod activity_service; // Legacy, to be replaced by logging_service
pub mod logging_service; // NEW
pub mod referral_service; // NEW
pub mod redis_service; // NEW
pub mod pubsub_service; // NEW
pub mod analytics_service;
pub mod monitoring;
pub mod traffic_service;
pub mod connection_service;
pub mod export_service;  // NEW: Database and settings export/backup
pub mod notification_service;
pub mod infrastructure_service;
pub mod security_service;
pub mod telemetry_service; // Added Phase 3

// Enterprise Modular Services
pub mod user_service;
pub mod billing_service;
pub mod subscription_service;
pub mod catalog_service;
pub mod generator_service;
pub mod org_service;
pub mod payment;
pub mod rotation_service;
pub mod update_service;

    Checking caramba-panel v0.3.0 (/Users/smtcprdx/Documents/exarobot/apps/caramba-panel)
warning: unused import: `warn`
 --> apps/caramba-sub/src/handlers/subscription.rs:7:28
  |
7 | use tracing::{info, error, warn};
  |                            ^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `rust_embed::RustEmbed`
 --> apps/caramba-sub/src/handlers/app.rs:6:5
  |
6 | use rust_embed::RustEmbed;
  |     ^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `std::sync::Arc`
 --> apps/caramba-bot/src/main.rs:4:5
  |
4 | use std::sync::Arc;
  |     ^^^^^^^^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `tokio::sync::RwLock`
 --> apps/caramba-bot/src/main.rs:5:5
  |
5 | use tokio::sync::RwLock;
  |     ^^^^^^^^^^^^^^^^^^^

warning: unused import: `std::collections::HashMap`
 --> apps/caramba-bot/src/main.rs:6:5
  |
6 | use std::collections::HashMap;
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `std::sync::Arc`
 --> apps/caramba-bot/src/api_client.rs:4:5
  |
4 | use std::sync::Arc;
  |     ^^^^^^^^^^^^^^

warning: unused imports: `GiftCode` and `Subscription`
 --> apps/caramba-bot/src/bot/handlers/command.rs:7:40
  |
7 | use crate::models::store::{User, Plan, Subscription, DetailedSubscription, SubscriptionIpTracking, CartItem, StoreCategory, GiftCode};
  |                                        ^^^^^^^^^^^^                                                                         ^^^^^^^^

warning: unused variable: `e`
   --> apps/caramba-bot/src/bot/handlers/callback.rs:368:29
    |
368 |                         Err(e) => {
    |                             ^ help: if this is intentional, prefix it with an underscore: `_e`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: struct `User` is never constructed
  --> apps/caramba-bot/src/api_client.rs:14:12
   |
14 | pub struct User {
   |            ^^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: struct `Plan` is never constructed
  --> apps/caramba-bot/src/api_client.rs:25:12
   |
25 | pub struct Plan {
   |            ^^^^

warning: struct `Subscription` is never constructed
  --> apps/caramba-bot/src/api_client.rs:35:12
   |
35 | pub struct Subscription {
   |            ^^^^^^^^^^^^

warning: methods `get_user` and `create_user` are never used
  --> apps/caramba-bot/src/api_client.rs:80:18
   |
42 | impl ApiClient {
   | -------------- methods in this implementation
...
80 |     pub async fn get_user(&self, telegram_id: i64) -> Result<Option<User>> {
   |                  ^^^^^^^^
...
98 |     pub async fn create_user(&self, user: &User) -> Result<User> {
   |                  ^^^^^^^^^^^

warning: fields `api` and `admin_service` are never read
  --> apps/caramba-bot/src/state.rs:11:9
   |
10 | pub struct AppState {
   |            -------- fields in this struct
11 |     pub api: ApiClient,
   |         ^^^
...
16 |     pub admin_service: AdminService,
   |         ^^^^^^^^^^^^^
   |
   = note: `AppState` has a derived impl for the trait `Clone`, but this is intentionally ignored during dead code analysis

warning: methods `get_setting` and `purchase_product_with_balance` are never used
   --> apps/caramba-bot/src/services/store_service.rs:99:18
    |
 13 | impl StoreService {
    | ----------------- methods in this implementation
...
 99 |     pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
    |                  ^^^^^^^^^^^
...
195 |     pub async fn purchase_product_with_balance(&self, user_id: i64, product_id: i64) -> Result<Product> {
    |                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: field `api` is never read
 --> apps/caramba-bot/src/services/admin_service.rs:7:5
  |
6 | pub struct AdminService {
  |            ------------ field in this struct
7 |     api: ApiClient,
  |     ^^^
  |
  = note: `AdminService` has a derived impl for the trait `Clone`, but this is intentionally ignored during dead code analysis

warning: methods `is_admin`, `get_sni_logs`, and `get_sni_pool` are never used
  --> apps/caramba-bot/src/services/admin_service.rs:15:18
   |
10 | impl AdminService {
   | ----------------- methods in this implementation
...
15 |     pub async fn is_admin(&self, tg_id: i64) -> bool {
   |                  ^^^^^^^^
...
31 |     pub async fn get_sni_logs(&self) -> Result<Vec<SniRotationLog>> {
   |                  ^^^^^^^^^^^^
...
35 |     pub async fn get_sni_pool(&self) -> Result<Vec<SniPool>> {
   |                  ^^^^^^^^^^^^

warning: `caramba-sub` (bin "caramba-sub") generated 2 warnings (run `cargo fix --bin "caramba-sub" -p caramba-sub` to apply 1 suggestion)
warning: `caramba-bot` (bin "caramba-bot") generated 14 warnings (run `cargo fix --bin "caramba-bot" -p caramba-bot` to apply 6 suggestions)
error[E0432]: unresolved import `crate::bot_manager`
 --> apps/caramba-panel/src/services/pay_service.rs:8:12
  |
8 | use crate::bot_manager::BotManager;
  |            ^^^^^^^^^^^ could not find `bot_manager` in the crate root

error[E0432]: unresolved import `crate::services::subscription_service::AlertType`
 --> apps/caramba-panel/src/services/monitoring.rs:4:45
  |
4 | use crate::services::subscription_service::{AlertType, RenewalResult};
  |                                             ^^^^^^^^^ no `AlertType` in `services::subscription_service`
  |
  = help: consider importing this enum instead:
          caramba_db::models::store::AlertType

error[E0432]: unresolved import `crate::bot_manager`
 --> apps/caramba-panel/src/services/telemetry_service.rs:8:12
  |
8 | use crate::bot_manager::BotManager;
  |            ^^^^^^^^^^^ could not find `bot_manager` in the crate root

error[E0432]: unresolved import `bot_manager`
  --> apps/caramba-panel/src/main.rs:23:5
   |
23 | use bot_manager::BotManager;
   |     ^^^^^^^^^^^ use of unresolved module or unlinked crate `bot_manager`
   |
help: to make use of source file apps/caramba-panel/src/bot_manager.rs, use `mod bot_manager` in this file to declare the module
   |
 1 + mod bot_manager;
   |

error[E0412]: cannot find type `AlertType` in this scope
   --> apps/caramba-panel/src/services/subscription_service.rs:743:67
    |
743 |     pub async fn check_and_send_alerts(&self) -> Result<Vec<(i64, AlertType)>> {
    |                                                                   ^^^^^^^^^ not found in this scope
    |
help: consider importing this enum
    |
  1 + use caramba_db::models::store::AlertType;
    |

error[E0603]: enum import `RenewalResult` is private
   --> apps/caramba-panel/src/services/monitoring.rs:4:56
    |
  4 | use crate::services::subscription_service::{AlertType, RenewalResult};
    |                                                        ^^^^^^^^^^^^^ private enum import
    |
note: the enum import `RenewalResult` is defined here...
   --> apps/caramba-panel/src/services/subscription_service.rs:3:102
    |
  3 | use caramba_db::models::store::{Plan, Subscription, SubscriptionWithDetails, GiftCode, PlanDuration, RenewalResult, SubscriptionIpTracking};
    |                                                                                                      ^^^^^^^^^^^^^
note: ...and refers to the enum `RenewalResult` which is defined here
   --> /Users/smtcprdx/Documents/exarobot/libs/caramba-db/src/models/store.rs:183:1
    |
183 | pub enum RenewalResult {
    | ^^^^^^^^^^^^^^^^^^^^^^ you could import this directly
help: import `RenewalResult` through the re-export
    |
  4 | use crate::services::subscription_service::{AlertType, models::store::RenewalResult};
    |                                                        +++++++++++++++

warning: unused imports: `error` and `info`
 --> apps/caramba-panel/src/services/store_service.rs:4:15
  |
4 | use tracing::{info, error};
  |               ^^^^  ^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused imports: `AlertType`, `DetailedReferral`, `RenewalResult`, and `SubscriptionWithDetails`
  --> apps/caramba-panel/src/services/store_service.rs:10:5
   |
10 |     RenewalResult, AlertType, DetailedReferral, SubscriptionWithDetails, CartItem
   |     ^^^^^^^^^^^^^  ^^^^^^^^^  ^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `caramba_db::models::network::InboundType`
  --> apps/caramba-panel/src/services/store_service.rs:12:5
   |
12 | use caramba_db::models::network::InboundType;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `self`
 --> apps/caramba-panel/src/services/subscription_service.rs:6:46
  |
6 | use crate::singbox::subscription_generator::{self, NodeInfo, UserKeys};
  |                                              ^^^^

warning: unused import: `Path`
 --> apps/caramba-panel/src/handlers/api/bot.rs:2:22
  |
2 |     extract::{State, Path},
  |                      ^^^^

warning: unused import: `Mutex`
  --> apps/caramba-panel/src/main.rs:17:22
   |
17 | use std::sync::{Arc, Mutex};
   |                      ^^^^^

warning: unused import: `std::collections::HashMap`
  --> apps/caramba-panel/src/main.rs:18:5
   |
18 | use std::collections::HashMap;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `std::time::Instant`
  --> apps/caramba-panel/src/main.rs:19:5
   |
19 | use std::time::Instant;
   |     ^^^^^^^^^^^^^^^^^^

warning: variable does not need to be mutable
   --> apps/caramba-panel/src/services/store_service.rs:218:13
    |
218 |         let mut tx = self.pool.begin().await?;
    |             ----^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/services/pay_service.rs:238:9
    |
238 |         self.bot_manager.get_username().await.unwrap_or_else(|| "YOUR_BOT_USERNAME".to_string())
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0308]: mismatched types
   --> apps/caramba-panel/src/services/monitoring.rs:182:13
    |
182 |         for (user_id, alert_type, _sub_id) in alerts {
    |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^    ------ this is an iterator with items of type `(i64, AlertType)`
    |             |
    |             expected a tuple with 2 elements, found one with 3 elements
    |
    = note: expected tuple `(i64, AlertType)`
               found tuple `(_, _, _)`

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/services/telemetry_service.rs:161:37
    |
161 |                  if let Some(bot) = self.bot_manager.get_bot().await.ok() {
    |                                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0560]: struct `UserKeys` has no field named `awg_private_key`
   --> apps/caramba-panel/src/services/subscription_service.rs:735:13
    |
735 |             awg_private_key: awg_private_key.clone(),
    |             ^^^^^^^^^^^^^^^ unknown field
    |
help: a field with a similar name exists
    |
735 |             _awg_private_key: awg_private_key.clone(),
    |             +

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/api/v2/node.rs:369:24
    |
369 |     if let Some(bot) = state.bot_manager.get_bot().await.ok() {
    |                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/api/client.rs:289:42
    |
289 |     let subs = match state.store_service.get_user_subscriptions(user_id).await {
    |                                          ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/api/client.rs:289:9
    |
289 |     let subs = match state.store_service.get_user_subscriptions(user_id).await {
    |         ^^^^
...
312 |     let result: Vec<serde_json::Value> = subs.iter().map(|s| {
    |                                          ---- type must be known at this point
    |
help: consider giving `subs` an explicit type
    |
289 |     let subs: /* Type */ = match state.store_service.get_user_subscriptions(user_id).await {
    |             ++++++++++++

error[E0599]: no method named `update_subscription_node` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/api/client.rs:836:39
    |
836 |             match state.store_service.update_subscription_node(sub_id, Some(body.node_id)).await {
    |                                       ^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `update_subscription_note` with a similar name
    |
836 -             match state.store_service.update_subscription_node(sub_id, Some(body.node_id)).await {
836 +             match state.store_service.update_subscription_note(sub_id, Some(body.node_id)).await {
    |

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/subscription.rs:97:50
    |
 97 |     let plan_details = match state.store_service.get_user_subscriptions(sub.user_id).await {
    |                                                  ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
  --> apps/caramba-panel/src/subscription.rs:99:13
   |
99 |             subs.iter()
   |             ^^^^ cannot infer type

error[E0599]: no method named `generate_clash` found for struct `Arc<SubscriptionService>` in the current scope
   --> apps/caramba-panel/src/subscription.rs:409:46
    |
409 |             match state.subscription_service.generate_clash(&sub, &node_infos, &user_keys) {
    |                                              ^^^^^^^^^^^^^^ method not found in `Arc<SubscriptionService>`

error[E0599]: no method named `generate_v2ray` found for struct `Arc<SubscriptionService>` in the current scope
   --> apps/caramba-panel/src/subscription.rs:418:47
    |
418 |              match state.subscription_service.generate_v2ray(&sub, &node_infos, &user_keys) {
    |                                               ^^^^^^^^^^^^^^ method not found in `Arc<SubscriptionService>`

error[E0599]: no method named `generate_singbox` found for struct `Arc<SubscriptionService>` in the current scope
   --> apps/caramba-panel/src/subscription.rs:427:47
    |
427 |              match state.subscription_service.generate_singbox(&sub, &node_infos, &user_keys) {
    |                                               ^^^^^^^^^^^^^^^^ method not found in `Arc<SubscriptionService>`

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:407:10
    |
407 |     let (content, content_type, filename) = match client_type {
    |          ^^^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:428:20
    |
428 |                 Ok(c) => (c, "application/json", "config.json"),
    |                    ^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:428:17
    |
428 |                 Ok(c) => (c, "application/json", "config.json"),
    |                 ^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
note: required by a bound in `std::prelude::v1::Ok`
   --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/result.rs:561:5

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:419:20
    |
419 |                 Ok(c) => (c, "text/plain", "config.txt"),
    |                    ^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:419:17
    |
419 |                 Ok(c) => (c, "text/plain", "config.txt"),
    |                 ^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
note: required by a bound in `std::prelude::v1::Ok`
   --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/result.rs:561:5

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:410:20
    |
410 |                 Ok(c) => (c, "application/yaml", "config.yaml"),
    |                    ^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:410:17
    |
410 |                 Ok(c) => (c, "application/yaml", "config.yaml"),
    |                 ^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
note: required by a bound in `std::prelude::v1::Ok`
   --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/result.rs:561:5

error[E0599]: no method named `into_response` found for tuple `(reqwest::StatusCode, [(HeaderName, &str); 4], str)` in the current scope
   --> apps/caramba-panel/src/subscription.rs:449:7
    |
440 | /     (
441 | |         StatusCode::OK,
442 | |         [
443 | |             (header::CONTENT_TYPE, content_type),
...   |
448 | |         content
449 | |     ).into_response()
    | |      -^^^^^^^^^^^^^ method not found in `(reqwest::StatusCode, [(HeaderName, &str); 4], str)`
    | |______|
    |
    |
help: some of the expressions' fields have a method of the same name
    |
449 |     ).0.into_response()
    |       ++
449 |     ).1.into_response()
    |       ++
449 |     ).2.into_response()
    |       ++

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/subscription.rs:440:5
    |
440 | /     (
441 | |         StatusCode::OK,
442 | |         [
443 | |             (header::CONTENT_TYPE, content_type),
...   |
448 | |         content
449 | |     ).into_response()
    | |_____^ doesn't have a size known at compile-time
    |
    = help: within `(reqwest::StatusCode, [(HeaderName, &str); 4], str)`, the trait `Sized` is not implemented for `str`
    = note: required because it appears within the type `(reqwest::StatusCode, [(HeaderName, &str); 4], str)`
    = note: tuples must have a statically known size to be initialized

error[E0599]: no method named `get_user_referrals` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/handlers/admin/users.rs:185:41
    |
185 |     let referrals = state.store_service.get_user_referrals(id).await.unwrap_or_default();
    |                                         ^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_user_nodes` with a similar name
    |
185 -     let referrals = state.store_service.get_user_referrals(id).await.unwrap_or_default();
185 +     let referrals = state.store_service.get_user_nodes(id).await.unwrap_or_default();
    |

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/handlers/admin/users.rs:185:21
    |
185 |     let referrals = state.store_service.get_user_referrals(id).await.unwrap_or_default();
    |                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_user_referral_earnings` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/handlers/admin/users.rs:186:46
    |
186 |     let earnings_cents = state.store_service.get_user_referral_earnings(id).await.unwrap_or(0);
    |                                              ^^^^^^^^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/handlers/admin/users.rs:186:26
    |
186 |     let earnings_cents = state.store_service.get_user_referral_earnings(id).await.unwrap_or(0);
    |                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `delete_plan_and_refund` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/handlers/admin/plans.rs:165:31
    |
165 |     match state.store_service.delete_plan_and_refund(id).await {
    |                               ^^^^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0599]: no method named `update_trial_plan_limits` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/handlers/admin/settings.rs:590:41
    |
590 |     if let Err(e) = state.store_service.update_trial_plan_limits(form.free_trial_device_limit, form.free_trial_traffic_limit).await {
    |                                         ^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `is_trial_plan` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:770:5
    |
770 |     pub async fn is_trial_plan(&self, id: i64) -> Result<bool> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0061]: this function takes 3 arguments but 2 arguments were supplied
   --> apps/caramba-panel/src/main.rs:428:30
    |
428 |           let connection_svc = services::connection_service::ConnectionService::new(
    |  ______________________________^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^-
429 | |             connection_orch,
430 | |             connection_store,
431 | |         );
    | |_________- argument #3 of type `Arc<SubscriptionService>` is missing
    |
note: associated function defined here
   --> apps/caramba-panel/src/services/connection_service.rs:63:12
    |
 63 |     pub fn new(
    |            ^^^
...
 66 |         subscription: Arc<SubscriptionService>,
    |         --------------------------------------
help: provide the argument
    |
428 |         let connection_svc = services::connection_service::ConnectionService::new(
429 |             connection_orch,
430 |             connection_store,
431 ~             /* Arc<SubscriptionService> */,
432 ~         );
    |

warning: unused import: `Row`
 --> apps/caramba-panel/src/services/store_service.rs:1:20
  |
1 | use sqlx::{PgPool, Row};
  |                    ^^^

warning: unused import: `sqlx::Row`
 --> apps/caramba-panel/src/services/catalog_service.rs:5:5
  |
5 | use sqlx::Row;
  |     ^^^^^^^^^

warning: unused import: `rust_embed::RustEmbed`
 --> apps/caramba-panel/src/handlers/local_app.rs:6:5
  |
6 | use rust_embed::RustEmbed;
  |     ^^^^^^^^^^^^^^^^^^^^^

warning: unused variable: `state`
  --> apps/caramba-panel/src/handlers/api/bot.rs:52:11
   |
52 |     State(state): State<AppState>,
   |           ^^^^^ help: if this is intentional, prefix it with an underscore: `_state`
   |
   = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `payload`
  --> apps/caramba-panel/src/handlers/api/bot.rs:53:10
   |
53 |     Json(payload): Json<VerifyUserRequest>,
   |          ^^^^^^^ help: if this is intentional, prefix it with an underscore: `_payload`

Some errors have detailed explanations: E0061, E0277, E0282, E0308, E0412, E0432, E0560, E0599, E0603.
For more information about an error, try `rustc --explain E0061`.
warning: `caramba-panel` (bin "caramba-panel") generated 14 warnings
error: could not compile `caramba-panel` (bin "caramba-panel") due to 35 previous errors; 14 warnings emitted

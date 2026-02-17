    Checking caramba-panel v0.3.0 (/Users/smtcprdx/Documents/exarobot/apps/caramba-panel)
    Checking caramba-bot v0.3.0 (/Users/smtcprdx/Documents/exarobot/apps/caramba-bot)
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

warning: `caramba-sub` (bin "caramba-sub") generated 2 warnings (run `cargo fix --bin "caramba-sub" -p caramba-sub` to apply 1 suggestion)
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

warning: `caramba-bot` (bin "caramba-bot") generated 14 warnings (run `cargo fix --bin "caramba-bot" -p caramba-bot` to apply 6 suggestions)
error[E0599]: no method named `set_user_referrer` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:291:56
    |
291 | ...                   match state.store_service.set_user_referrer(u.id, ref_code).await {
    |                                                 ^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/command.rs:293:127
    |
293 | ...   Err(e) => { let _ = bot.send_message(msg.chat.id, format!("❌ Linking Failed: {}", escape_md(&e.to_string()))).parse_mode(ParseMode::...
    |                                                                                                     ^ cannot infer type

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:545:58
    |
545 |                     let subs = match state.store_service.get_user_subscriptions(user.id).await {
    |                                                          ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `get_referral_count` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:688:62
    |
688 |                     let ref_count: i64 = state.store_service.get_referral_count(user.id).await.unwrap_or(0);
    |                                                              ^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/command.rs:688:42
    |
688 |                     let ref_count: i64 = state.store_service.get_referral_count(user.id).await.unwrap_or(0);
    |                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_user_referral_earnings` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:689:65
    |
689 |                     let ref_earnings: i64 = state.store_service.get_user_referral_earnings(user.id).await.unwrap_or(0);
    |                                                                 ^^^^^^^^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/command.rs:689:45
    |
689 |                     let ref_earnings: i64 = state.store_service.get_user_referral_earnings(user.id).await.unwrap_or(0);
    |                                             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:759:62
    |
759 |                        if let Ok(subs) = state.store_service.get_user_subscriptions(u.id).await {
    |                                                              ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/command.rs:760:103
    |
760 | ...   let active_subs: Vec<caramba_db::models::store::SubscriptionWithDetails> = subs.into_iter().filter(|s| s.sub.status == "active").co...
    |                                                                                  ^^^^ cannot infer type

error[E0599]: no method named `get_subscription_active_ips` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:773:118
    |
773 | ...   let ips: Vec<caramba_db::models::store::SubscriptionIpTracking> = state.store_service.get_subscription_active_ips(sub.sub.id).await...
    |                                                                                             ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/command.rs:773:98
    |
773 | ...ubscriptionIpTracking> = state.store_service.get_subscription_active_ips(sub.sub.id).await.unwrap_or_default();
    |                             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_subscription_device_limit` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:774:69
    |
774 | ...                   let limit: i64 = state.store_service.get_subscription_device_limit(sub.sub.id).await.unwrap_or(0).into();
    |                                                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/command.rs:774:49
    |
774 | ...                   let limit: i64 = state.store_service.get_subscription_device_limit(sub.sub.id).await.unwrap_or(0).into();
    |                                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `num_minutes` found for struct `chrono::DateTime<Tz>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/command.rs:784:60
    |
784 | ...                   let mins = time_ago.num_minutes();
    |                                           ^^^^^^^^^^^
    |
help: there is a method `minute` with a similar name
    |
784 -                                        let mins = time_ago.num_minutes();
784 +                                        let mins = time_ago.minute();
    |

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:290:63
    |
290 |                         if let Ok(subs) = state.store_service.get_user_subscriptions(user_tg.id).await {
    |                                                               ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:291:43
    |
291 | ...                   let sub_opt = subs.iter().find(|s| s.sub.id == sub_id);
    |                                     ^^^^ cannot infer type

error[E0599]: no method named `get_subscription_links` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:295:59
    |
295 | ...                   match state.store_service.get_subscription_links(sub_id).await {
    |                                                 ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:297:44
    |
297 | ...                   if links.is_empty() {
    |                          ^^^^^ cannot infer type

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/bot/handlers/callback.rs:332:49
    |
332 | ...                   for link in links {
    |                           ^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
    = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/bot/handlers/callback.rs:332:45
    |
332 | / ...                   for link in links {
333 | | ...                       response.push_str(&format!("`{}`\n\n", escape_md(&link)));
334 | | ...                   }
    | |_______________________^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
note: required by a bound in `std::prelude::v1::None`
   --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/option.rs:603:5

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/bot/handlers/callback.rs:332:57
    |
332 | ...                   for link in links {
    |                                   ^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
note: required by an implicit `Sized` bound in `std::option::Option`
   --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/option.rs:599:1

error[E0599]: no method named `generate_subscription_file` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:356:47
    |
356 |                     match state.store_service.generate_subscription_file(u.id).await {
    |                                               ^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:358:40
    |
358 | ...                   let data = json_content.into_bytes();
    |                                  ^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_subscription_device_limit` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:476:60
    |
476 |                     let device_limit = state.store_service.get_subscription_device_limit(sub_id).await.unwrap_or(3);
    |                                                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:476:40
    |
476 |                     let device_limit = state.store_service.get_subscription_device_limit(sub_id).await.unwrap_or(3);
    |                                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_subscription_active_ips` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:479:58
    |
479 |                     let active_ips = state.store_service.get_subscription_active_ips(sub_id).await.unwrap_or_default();
    |                                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:479:38
    |
479 |                     let active_ips = state.store_service.get_subscription_active_ips(sub_id).await.unwrap_or_default();
    |                                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `num_minutes` found for struct `chrono::DateTime<Tz>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:492:56
    |
492 | ...                   let minutes_ago = time_ago.num_minutes();
    |                                                  ^^^^^^^^^^^
    |
help: there is a method `minute` with a similar name
    |
492 -                             let minutes_ago = time_ago.num_minutes();
492 +                             let minutes_ago = time_ago.minute();
    |

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:586:59
    |
586 |                     if let Ok(subs) = state.store_service.get_user_subscriptions(user.id).await {
    |                                                           ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:588:47
    |
588 |                         let mut sorted_subs = subs.clone();
    |                                               ^^^^ cannot infer type

error[E0599]: no method named `purchase_product_with_balance` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:767:51
    |
767 |                         match state.store_service.purchase_product_with_balance(u.id, prod_id).await {
    |                                                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0277]: the size for values of type `str` cannot be known at compilation time
   --> apps/caramba-panel/src/bot/handlers/callback.rs:771:44
    |
771 | ...                   if let Some(content) = product.content {
    |                              ^^^^^^^^^^^^^ doesn't have a size known at compile-time
    |
    = help: the trait `Sized` is not implemented for `str`
note: required by a bound in `std::prelude::v1::Some`
   --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/option.rs:607:5

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:796:59
    |
796 |                       let user_subs = state.store_service.get_user_subscriptions(u.id).await.unwrap_or_default();
    |                                                           ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:796:39
    |
796 |                       let user_subs = state.store_service.get_user_subscriptions(u.id).await.unwrap_or_default();
    |                                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_subscription_active_ips` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:799:57
    |
799 |                           let ips = state.store_service.get_subscription_active_ips(sub_id).await.unwrap_or_default();
    |                                                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:799:37
    |
799 |                           let ips = state.store_service.get_subscription_active_ips(sub_id).await.unwrap_or_default();
    |                                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_subscription_device_limit` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:800:59
    |
800 |                           let limit = state.store_service.get_subscription_device_limit(sub_id).await.unwrap_or(0);
    |                                                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:800:39
    |
800 |                           let limit = state.store_service.get_subscription_device_limit(sub_id).await.unwrap_or(0);
    |                                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `num_minutes` found for struct `chrono::DateTime<Tz>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:812:55
    |
812 | ...                   let mins = time_ago.num_minutes();
    |                                           ^^^^^^^^^^^
    |
help: there is a method `minute` with a similar name
    |
812 -                                   let mins = time_ago.num_minutes();
812 +                                   let mins = time_ago.minute();
    |

error[E0599]: no method named `get_user_subscriptions` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:842:59
    |
842 |                       let user_subs = state.store_service.get_user_subscriptions(u.id).await.unwrap_or_default();
    |                                                           ^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/bot/handlers/callback.rs:842:39
    |
842 |                       let user_subs = state.store_service.get_user_subscriptions(u.id).await.unwrap_or_default();
    |                                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ cannot infer type

error[E0599]: no method named `get_product` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/bot/handlers/callback.rs:901:70
    |
901 | ...                   if let Ok(product) = state.store_service.get_product(prod_id).await {
    |                                                                ^^^^^^^^^^^
    |
help: there is a method `get_all_products` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:705:5
    |
705 |     pub async fn get_all_products(&self) -> Result<Vec<caramba_db::models::store::Product>> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0277]: `()` is not an iterator
    --> apps/caramba-panel/src/bot/handlers/callback.rs:1006:42
     |
1006 | ...                   for note in notes {
     |                                   ^^^^^ `()` is not an iterator
     |
     = help: the trait `Iterator` is not implemented for `()`
     = note: required for `()` to implement `IntoIterator`

error[E0277]: the size for values of type `str` cannot be known at compilation time
    --> apps/caramba-panel/src/bot/handlers/callback.rs:1006:34
     |
1006 | ...                   for note in notes {
     |                           ^^^^ doesn't have a size known at compile-time
     |
     = help: the trait `Sized` is not implemented for `str`
     = note: all local variables must have a statically known size

error[E0277]: the size for values of type `str` cannot be known at compilation time
    --> apps/caramba-panel/src/bot/handlers/callback.rs:1006:30
     |
1006 | / ...                   for note in notes {
1007 | | ...                       response.push_str(&format!("{}\n", escape_md(&note)));
1008 | | ...                   }
     | |_______________________^ doesn't have a size known at compile-time
     |
     = help: the trait `Sized` is not implemented for `str`
note: required by a bound in `std::prelude::v1::None`
    --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/option.rs:603:5

error[E0277]: the size for values of type `str` cannot be known at compilation time
    --> apps/caramba-panel/src/bot/handlers/callback.rs:1006:42
     |
1006 | ...                   for note in notes {
     |                                   ^^^^^ doesn't have a size known at compile-time
     |
     = help: the trait `Sized` is not implemented for `str`
note: required by an implicit `Sized` bound in `std::option::Option`
    --> /private/tmp/rust-20251211-8300-9xlhcz/rustc-1.92.0-src/library/core/src/option.rs:599:1

error[E0599]: no method named `toggle_auto_renewal` found for struct `Arc<StoreService>` in the current scope
    --> apps/caramba-panel/src/bot/handlers/callback.rs:1074:43
     |
1074 |                 match state.store_service.toggle_auto_renewal(sub_id).await {
     |                                           ^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`
     |
help: one of the expressions' fields has a method of the same name
     |
1074 |                 match state.store_service.sub_repo.toggle_auto_renewal(sub_id).await {
     |                                           +++++++++

error[E0599]: no method named `enable_ctrlc_handler` found for struct `DispatcherBuilder<R, Err, Key>` in the current scope
  --> apps/caramba-panel/src/bot/mod.rs:52:10
   |
44 |       let mut dispatcher = Dispatcher::builder(bot, dptree::entry()
   |  __________________________-
45 | |         .branch(handler)
46 | |         .branch(callback_handler)
47 | |         .branch(pre_checkout_handler))
...  |
51 | |         })
52 | |         .enable_ctrlc_handler()
   | |         -^^^^^^^^^^^^^^^^^^^^ method not found in `DispatcherBuilder<teloxide::Bot, RequestError, DefaultKey>`
   | |_________|
   |

warning: variable does not need to be mutable
   --> apps/caramba-panel/src/services/store_service.rs:218:13
    |
218 |         let mut tx = self.pool.begin().await?;
    |             ----^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

error[E0599]: no method named `process_order_payment` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/services/pay_service.rs:650:28
    |
650 |         self.store_service.process_order_payment(order_id).await?;
    |                            ^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `log_payment` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:684:5
    |
684 |     pub async fn log_payment(&self, user_id: i64, method: &str, amount_cents: i64, external_id: Option<&str>, status: &str) -> Result<()> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0599]: no method named `process_auto_renewals` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/services/monitoring.rs:114:48
    |
114 |         let results = self.state.store_service.process_auto_renewals().await?;
    |                                                ^^^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/services/monitoring.rs:114:13
    |
114 |         let results = self.state.store_service.process_auto_renewals().await?;
    |             ^^^^^^^
115 |         
116 |         if results.is_empty() {
    |            ------- type must be known at this point
    |
help: consider giving `results` an explicit type
    |
114 |         let results: /* Type */ = self.state.store_service.process_auto_renewals().await?;
    |                    ++++++++++++

error[E0599]: no method named `check_traffic_alerts` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/services/monitoring.rs:173:47
    |
173 |         let alerts = self.state.store_service.check_traffic_alerts().await?;
    |                                               ^^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/services/monitoring.rs:173:13
    |
173 |         let alerts = self.state.store_service.check_traffic_alerts().await?;
    |             ^^^^^^
174 |         
175 |         if alerts.is_empty() {
    |            ------ type must be known at this point
    |
help: consider giving `alerts` an explicit type
    |
173 |         let alerts: /* Type */ = self.state.store_service.check_traffic_alerts().await?;
    |                   ++++++++++++

error[E0599]: no method named `cleanup_old_ip_tracking` found for struct `Arc<StoreService>` in the current scope
  --> apps/caramba-panel/src/services/connection_service.rs:84:40
   |
84 |             if let Err(e) = self.store.cleanup_old_ip_tracking().await {
   |                                        ^^^^^^^^^^^^^^^^^^^^^^^ method not found in `Arc<StoreService>`

error[E0599]: no method named `get_subscription_device_limit` found for struct `Arc<StoreService>` in the current scope
   --> apps/caramba-panel/src/services/connection_service.rs:194:39
    |
194 |         let device_limit = self.store.get_subscription_device_limit(sub_id).await?;
    |                                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
help: there is a method `get_subscription` with a similar name, but with different arguments
   --> apps/caramba-panel/src/services/store_service.rs:426:5
    |
426 |     pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error[E0282]: type annotations needed
   --> apps/caramba-panel/src/services/connection_service.rs:194:13
    |
194 |         let device_limit = self.store.get_subscription_device_limit(sub_id).await?;
    |             ^^^^^^^^^^^^
...
215 |                 sub_id, active_device_count, if device_limit == 0 { "Unlimited".to_string() } else { device_limit.to_string() }
    |                                                                                                      ------------ type must be known at this point
    |
help: consider giving `device_limit` an explicit type
    |
194 |         let device_limit: /* Type */ = self.store.get_subscription_device_limit(sub_id).await?;
    |                         ++++++++++++

error[E0277]: the trait bound `std::option::Option<u32>: sqlx::Encode<'_, _>` is not satisfied
  --> apps/caramba-panel/src/services/telemetry_service.rs:84:19
   |
84 |             .bind(active_connections)
   |              ---- ^^^^^^^^^^^^^^^^^^ the trait `sqlx::Encode<'_, _>` is not implemented for `std::option::Option<u32>`
   |              |
   |              required by a bound introduced by this call
   |
   = help: the following other types implement trait `sqlx::Encode<'q, DB>`:
             `std::option::Option<T>` implements `sqlx::Encode<'_, Postgres>`
             `std::option::Option<T>` implements `sqlx::Encode<'_, sqlx::Any>`
note: required by a bound in `sqlx::query::Query::<'q, DB, <DB as sqlx::Database>::Arguments<'q>>::bind`
  --> /Users/smtcprdx/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sqlx-core-0.8.6/src/query.rs:86:25
   |
86 |     pub fn bind<T: 'q + Encode<'q, DB> + Type<DB>>(mut self, value: T) -> Self {
   |                         ^^^^^^^^^^^^^^ required by this bound in `Query::<'q, DB, <DB as Database>::Arguments<'q>>::bind`

error[E0277]: the trait bound `u32: sqlx::Type<_>` is not satisfied
  --> apps/caramba-panel/src/services/telemetry_service.rs:84:19
   |
84 |             .bind(active_connections)
   |              ---- ^^^^^^^^^^^^^^^^^^ the trait `sqlx::Type<_>` is not implemented for `u32`
   |              |
   |              required by a bound introduced by this call
   |
   = help: the following other types implement trait `sqlx::Type<DB>`:
             `f32` implements `sqlx::Type<Postgres>`
             `f32` implements `sqlx::Type<sqlx::Any>`
             `f64` implements `sqlx::Type<Postgres>`
             `f64` implements `sqlx::Type<sqlx::Any>`
             `i16` implements `sqlx::Type<Postgres>`
             `i16` implements `sqlx::Type<sqlx::Any>`
             `i32` implements `sqlx::Type<Postgres>`
             `i32` implements `sqlx::Type<sqlx::Any>`
           and 3 others
   = note: required for `std::option::Option<u32>` to implement `sqlx::Type<_>`
note: required by a bound in `sqlx::query::Query::<'q, DB, <DB as sqlx::Database>::Arguments<'q>>::bind`
  --> /Users/smtcprdx/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sqlx-core-0.8.6/src/query.rs:86:42
   |
86 |     pub fn bind<T: 'q + Encode<'q, DB> + Type<DB>>(mut self, value: T) -> Self {
   |                                          ^^^^^^^^ required by this bound in `Query::<'q, DB, <DB as Database>::Arguments<'q>>::bind`
help: use a unary tuple instead
   |
84 |             .bind((active_connections,))
   |                   +                  ++

error[E0599]: no method named `unwrap_or` found for type `bool` in the current scope
   --> apps/caramba-panel/src/services/subscription_service.rs:275:14
    |
271 |           let current: bool = sqlx::query_scalar::<_, bool>("SELECT auto_renew FROM subscriptions WHERE id = $1")
    |  _____________________________-
272 | |             .bind(subscription_id)
273 | |             .fetch_one(&self.pool)
274 | |             .await?
275 | |             .unwrap_or(false);
    | |             -^^^^^^^^^ method not found in `bool`
    | |_____________|
    |

error[E0560]: struct `UserKeys` has no field named `awg_private_key`
   --> apps/caramba-panel/src/services/subscription_service.rs:736:13
    |
736 |             awg_private_key,
    |             ^^^^^^^^^^^^^^^ unknown field
    |
help: a field with a similar name exists
    |
736 |             _awg_private_key,
    |             +

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

Some errors have detailed explanations: E0277, E0282, E0560, E0599.
For more information about an error, try `rustc --explain E0277`.
warning: `caramba-panel` (bin "caramba-panel") generated 14 warnings
error: could not compile `caramba-panel` (bin "caramba-panel") due to 83 previous errors; 14 warnings emitted

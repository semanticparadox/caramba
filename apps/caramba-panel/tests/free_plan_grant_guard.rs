//! Текстовые инварианты выдачи бесплатного плана.
//!
//! Всё, что здесь проверяется, полностью видно в исходниках, а живой базы у
//! тестов этого крейта нет (CI гоняет `cargo test` без Postgres) — поэтому
//! проверки идут по тексту, в том же стиле, что и `sql_dialect_guard.rs`.
//!
//! Каждый из этих инвариантов ломается молча: подписка создаётся, показывается
//! в кабинете и не пускает ни в один inbound, либо бесплатный пользователь
//! получает десятикратную квоту. Ни один из отказов не виден ни в логах, ни в
//! ответе API — только по жалобе пользователя.

use std::fs;
use std::path::{Path, PathBuf};

fn panel_src(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Нормализует SQL-литерал: переносы строк и отступы в тексте запроса
/// незначимы, а проверять хочется состав колонок, а не форматирование.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Вырезает кусок исходника между двумя маркерами — чтобы утверждение
/// относилось к конкретному запросу, а не к «где-то в файле есть такая строка».
/// Пропавший маркер — это провал теста, а не тихий пропуск проверки.
fn between<'a>(haystack: &'a str, start: &str, end: &str) -> &'a str {
    let from = haystack
        .find(start)
        .unwrap_or_else(|| panic!("маркер `{start}` исчез из файла — обновите тест"));
    let rest = &haystack[from..];
    let to = rest[start.len()..]
        .find(end)
        .unwrap_or_else(|| panic!("маркер `{end}` исчез из файла — обновите тест"));
    &rest[..start.len() + to + end.len()]
}

/// Генерация конфигов нод (`orchestration_service`) выбирает `s.vless_uuid` и
/// молча пропускает подписки, где он NULL, а UPDATE, который проставил бы его
/// задним числом, в коде отсутствует. INSERT без `vless_uuid` создаёт строку,
/// которая существует везде, кроме единственного места, где она нужна.
#[test]
fn every_free_subscription_insert_binds_a_vless_uuid() {
    let store = panel_src("services/store_service.rs");
    let grant_fn = between(
        &store,
        "pub async fn ensure_free_plan_subscription_tx",
        "pub async fn ensure_free_plan_subscription(",
    );
    let insert = squash(between(
        grant_fn,
        "INSERT INTO subscriptions",
        "RETURNING id",
    ));
    assert!(
        insert.contains("vless_uuid"),
        "ensure_free_plan_subscription_tx создаёт подписку без vless_uuid — она не попадёт в конфиги нод: {insert}"
    );
    assert!(
        insert.contains("subscription_uuid"),
        "ensure_free_plan_subscription_tx потерял subscription_uuid: {insert}"
    );

    let bot_api = panel_src("handlers/api/bot.rs");
    let endpoint = between(
        &bot_api,
        "pub async fn create_free_subscription",
        "RETURNING id",
    );
    let free_insert = squash(between(
        endpoint,
        "INSERT INTO subscriptions",
        "RETURNING id",
    ));
    assert!(
        free_insert.contains("vless_uuid"),
        "create_free_subscription создаёт подписку без vless_uuid — эндпоинт вернёт id, но подключиться будет нельзя: {free_insert}"
    );
}

/// Реактивация истёкшей бесплатной подписки поднимает СТАРУЮ строку. Если та
/// была создана до того, как этот путь начал выставлять `vless_uuid`, «оживший»
/// пользователь остаётся невидимым для нод — навсегда, потому что второй
/// реактивации уже не будет.
#[test]
fn reactivating_a_free_subscription_heals_a_missing_vless_uuid() {
    let store = panel_src("services/store_service.rs");
    let grant_fn = between(
        &store,
        "pub async fn ensure_free_plan_subscription_tx",
        "pub async fn ensure_free_plan_subscription(",
    );
    let update = squash(between(grant_fn, "UPDATE subscriptions", "RETURNING id"));

    assert!(
        update.contains("vless_uuid"),
        "реактивация не лечит vless_uuid: {update}"
    );
    assert!(
        update.contains("COALESCE(NULLIF(vless_uuid, ''), gen_random_uuid()::TEXT)"),
        "реактивация обязана заполнять vless_uuid только когда он пуст, а не перезаписывать рабочий: {update}"
    );
}

/// Потолок бесплатного плана — `plans.daily_traffic_mb`, а не
/// `traffic_limit_gb`: последний приводится к BIGINT и не умеет выражать доли
/// гигабайта, поэтому 200 МБ/сутки в нём не помещаются. Если CASE схлопнуть
/// обратно в одно слагаемое, бесплатный пользователь тихо получит те 10 ГБ,
/// которые написаны в гигабайтной колонке.
#[test]
fn the_quota_ceiling_branches_on_is_free() {
    let bonus = panel_src("services/bonus_traffic.rs");
    let expr = squash(between(
        &bonus,
        "pub const QUOTA_LIMIT_BYTES_SQL",
        "pub const QUOTA_LIMITED_PLAN_SQL",
    ));

    assert!(
        expr.contains("p.is_free"),
        "потолок квоты не различает бесплатный и платный план: {expr}"
    );
    assert!(
        expr.contains("p.daily_traffic_mb") && expr.contains("1048576"),
        "бесплатная ветка обязана считаться от daily_traffic_mb в мегабайтах: {expr}"
    );
    // Платная ветка обязана остаться байт-в-байт прежней: правка бесплатного
    // тарифа не имеет права сдвинуть ни одну платную подписку.
    assert!(
        expr.contains("CAST(p.traffic_limit_gb AS BIGINT) * 1073741824"),
        "платная ветка потолка изменилась: {expr}"
    );
    assert!(
        expr.contains("COALESCE(u.bonus_traffic_mb, 0) * 1048576"),
        "бонусный трафик выпал из потолка: {expr}"
    );
}

/// Троттлинг и ночное снятие троттлинга обязаны сравнивать ОДНО выражение —
/// и одинаково понимать, у каких планов потолок вообще есть. Разошедшиеся
/// определения дают либо застревание в 'throttled', либо флап
/// 'active' ⇄ 'throttled' каждую ночь.
#[test]
fn throttle_and_unthrottle_share_the_same_ceiling() {
    let monitoring = panel_src("services/monitoring.rs");
    let subs = panel_src("services/subscription_service.rs");

    let unthrottle = between(
        &monitoring,
        "let reactivate_sql = format!(",
        "let reactivated_plan_ids",
    );
    let throttle = between(
        &subs,
        "pub async fn throttle_free_quota_subscriptions",
        "let rows =",
    );

    for (name, sql) in [("unthrottle", unthrottle), ("throttle", throttle)] {
        assert!(
            sql.contains("bonus_traffic::QUOTA_LIMIT_BYTES_SQL"),
            "{name} перестал использовать общий потолок QUOTA_LIMIT_BYTES_SQL"
        );
        assert!(
            sql.contains("bonus_traffic::QUOTA_LIMITED_PLAN_SQL"),
            "{name} завёл собственное представление о безлимите вместо QUOTA_LIMITED_PLAN_SQL"
        );
    }
}

/// Приём соглашения в боте — единственная дверь внутрь (`command.rs` заворачивает
/// любое сообщение, пока `terms_accepted_at` пуст), поэтому именно он обязан
/// выдавать бесплатный план. Без этого вызова пользователь получает меню, за
/// которым нет подписки.
#[test]
fn accepting_terms_in_the_bot_grants_the_free_plan() {
    let callback = panel_src("bot/handlers/callback.rs");
    let arm = between(&callback, "\"accept_terms\" =>", "\"decline_terms\" =>");

    assert!(
        arm.contains("update_user_terms"),
        "ветка accept_terms больше не отмечает принятие соглашения — тест устарел"
    );
    assert!(
        arm.contains("app_auth::grant_free_plan_on_signup"),
        "ветка accept_terms не выдаёт бесплатный план: аккаунт останется без подписки"
    );
}

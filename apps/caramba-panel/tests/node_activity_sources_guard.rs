//! Инварианты источников данных для онлайна, ёмкости и трафика узлов.
//!
//! Живой базы у тестов этого крейта нет (CI гоняет `cargo test` без Postgres),
//! поэтому проверки идут по тексту — в том же стиле, что `sql_dialect_guard.rs`
//! и `free_plan_grant_guard.rs`.
//!
//! Каждый инвариант здесь описывает отказ, который НЕ виден ни в логах, ни в
//! ответах: график рисуется, числа показываются, просто они не про то. Именно
//! так панель полгода показывала пустой Traffic History и алл-тайм трафик под
//! подписью «30 дней».

use std::fs;
use std::path::{Path, PathBuf};

fn panel_src(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn migration(name: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../libs/caramba-db/migrations")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Переносы и отступы в SQL незначимы: сравнивать хочется состав запроса.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Кусок исходника от маркера до маркера. Пропавший маркер — провал теста, а
/// не тихо пропущенная проверка.
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

/// График Traffic History считается по `app_traffic_daily`.
///
/// Регрессия, которая не должна вернуться: источником был
/// `daily_stats.traffic_used`, а инкремента этой колонки в коде нет ни одного
/// (`AnalyticsService::track_traffic` не вызывается ниоткуда). На проде таблица
/// пустая, график молча падал в фолбэк «Today = 0», и это выглядело как
/// «трафика нет», а не как «источник мёртв».
#[test]
fn traffic_history_reads_live_table_not_dead_daily_stats() {
    let src = panel_src("services/analytics_service.rs");
    let body = squash(between(
        &src,
        "pub async fn get_traffic_history",
        "Ok(history)",
    ));

    assert!(
        body.contains("FROM app_traffic_daily"),
        "get_traffic_history обязан читать app_traffic_daily: {body}"
    );
    assert!(
        !body.contains("FROM daily_stats"),
        "daily_stats.traffic_used никем не заполняется — график снова будет пустым: {body}"
    );
    // Окно ровно 30 дней, а не «сколько накопилось».
    assert!(
        body.contains("CURRENT_DATE - 29"),
        "окно графика должно быть 30 дней: {body}"
    );
}

/// Подпись «30 дней» на Dashboard/Analytics должна означать 30 дней.
///
/// Раньше в это поле клали алл-тайм сумму счётчиков узлов с комментарием
/// «Placeholder»: чем дольше жили узлы, тем сильнее врала цифра, и заметить это
/// по самой цифре было нельзя.
#[test]
fn thirty_day_traffic_is_a_real_window() {
    let src = panel_src("services/analytics_service.rs");
    let body = squash(between(
        &src,
        "let total_traffic_30d_bytes",
        "unwrap_or(0);",
    ));

    assert!(
        body.contains("FROM app_traffic_daily") && body.contains("CURRENT_DATE - 29"),
        "30-дневный трафик должен считаться окном по app_traffic_daily: {body}"
    );
    assert!(
        !squash(&src).contains("total_traffic_30d_bytes: total_traffic_bytes"),
        "30-дневное поле снова подменено алл-тайм счётчиком"
    );
}

/// Разбивка трафика по узлам берётся из снапшотов, а не из `subscriptions`.
///
/// `subscriptions.node_id` бывает NULL и указывает максимум на один узел из
/// нескольких, обслуживающих план, а `used_traffic` накопительный — сумма по
/// такому ключу структурно неверна для любого мультинодового плана.
#[test]
fn node_traffic_comes_from_snapshots() {
    let src = panel_src("services/analytics_service.rs");
    let body = squash(between(
        &src,
        "pub async fn get_node_traffic_stats",
        "Ok(nodes)",
    ));

    assert!(
        body.contains("FROM node_traffic_snapshots"),
        "трафик по узлам должен считаться по снапшотам: {body}"
    );
    assert!(
        !body.contains("s.used_traffic"),
        "накопительный subscriptions.used_traffic снова стал источником: {body}"
    );
}

/// Переустановка узла обнуляет накопительные счётчики.
///
/// Поэтому окно считается суммой ПОЛОЖИТЕЛЬНЫХ шагов между соседними замерами
/// (`GREATEST(... - LAG(...), 0)`), а не «последний минус первый»: после одного
/// рестарта такая разница ушла бы в минус и съела всю историю окна.
#[test]
fn snapshot_windows_survive_counter_resets() {
    for (file, marker, end) in [
        (
            "services/analytics_service.rs",
            "pub async fn get_node_traffic_stats",
            "Ok(nodes)",
        ),
        (
            "services/node_activity_service.rs",
            "pub async fn traffic_by_node",
            "Ok(rows",
        ),
    ] {
        let src = panel_src(file);
        let body = squash(between(&src, marker, end));
        assert!(
            body.contains("GREATEST(total_ingress - LAG(total_ingress)")
                && body.contains("GREATEST(total_egress - LAG(total_egress)"),
            "{file}: окно трафика обязано суммировать положительные шаги: {body}"
        );
        assert!(
            body.contains("PARTITION BY node_id ORDER BY ts"),
            "{file}: LAG без PARTITION BY смешает счётчики разных узлов: {body}"
        );
    }
}

/// «Сейчас» на узле всегда ограничено окном свежести.
///
/// В `node_user_activity` строка живёт вечно, а `online` — снимок последнего
/// heartbeat. Узел, который умер или потерял связь, иначе навсегда остался бы в
/// админке с полным залом народа, и отличить это от живого узла было бы нельзя.
#[test]
fn online_counters_are_always_time_bounded() {
    let src = panel_src("services/node_activity_service.rs");

    let counters = squash(between(&src, "pub async fn counters_by_node", "Ok(out)"));
    assert!(
        counters.contains("WHERE online AND last_seen_at > NOW() -"),
        "счётчик «Сейчас» обязан ограничиваться окном по last_seen_at: {counters}"
    );

    let list = squash(between(&src, "pub async fn node_user_list", "Ok(out)"));
    assert!(
        list.contains("last_seen_at > NOW() - ($2::int * INTERVAL '1 second')"),
        "список онлайна обязан ограничиваться окном по last_seen_at: {list}"
    );
}

/// Резолв тега в пользователя — пакетный, без запроса на строку.
///
/// Список онлайна открывается прямо из таблицы узлов, и N+1 здесь означает
/// сотни запросов на одно наведение мыши.
#[test]
fn tag_resolution_is_batched() {
    let src = panel_src("services/node_activity_service.rs");
    let body = squash(between(&src, "pub async fn resolve_user_tags", "Ok(out)"));

    assert!(
        body.contains("WHERE tg_id = ANY($1)") && body.contains("WHERE id = ANY($1)"),
        "резолв тегов должен идти пакетно через = ANY: {body}"
    );
    assert!(
        body.contains("s.vless_uuid = ANY($1) OR s.subscription_uuid = ANY($1)"),
        "теги-uuid тоже обязаны резолвиться пакетно: {body}"
    );
}

/// Снапшоты трафика снимаются и чистятся в том же десятиминутном цикле.
///
/// Без записи снапшотов все окна трафика по узлам молча станут нулевыми: SQL
/// отработает, просто данных не будет — ошибки не увидит никто.
#[test]
fn traffic_service_writes_and_prunes_snapshots() {
    let src = squash(&panel_src("services/traffic_service.rs"));
    assert!(
        src.contains("take_traffic_snapshot()"),
        "TrafficService перестал снимать снапшоты трафика узлов"
    );
    assert!(
        src.contains("purge_old_snapshots()"),
        "TrafficService перестал чистить снапшоты — таблица будет расти вечно"
    );
}

/// Недоступность Clash API больше не считается аварией.
///
/// У одного из узлов порт 9090 закрыт снаружи хостером: прежний код писал
/// ERROR каждые пять минут про один и тот же известный факт и топил в этом
/// настоящие ошибки. Онлайн считается не здесь, поэтому отказ опроса — это
/// строка в часовом отчёте, а не ошибка.
#[test]
fn clash_polling_failures_are_not_errors_anymore() {
    let src = panel_src("services/connection_service.rs");
    assert!(
        !src.contains("error!(\"Failed to fetch connections from node"),
        "отказ опроса узла снова пишется как ERROR каждые пять минут"
    );
    assert!(
        src.contains("DIAG_REPORT_INTERVAL") && src.contains("fn note_cycle"),
        "пропал часовой отчёт со счётчиками резолва — резолв снова чёрный ящик"
    );
}

/// Миграция волны строго аддитивная.
///
/// Панель и нода релизятся с main, откат идёт на предыдущий тег: любая
/// разрушающая операция в миграции делает откат невозможным.
#[test]
fn wave_migration_is_additive_only() {
    let sql = migration("20260911130000_node_activity_snapshots.sql");
    let upper = sql.to_uppercase();

    assert!(upper.contains("CREATE TABLE IF NOT EXISTS NODE_TRAFFIC_SNAPSHOTS"));
    assert!(upper.contains("ADD COLUMN IF NOT EXISTS MAX_USERS_OVERRIDE"));
    for forbidden in ["DROP TABLE", "DROP COLUMN", "TRUNCATE", "ALTER COLUMN"] {
        assert!(
            !upper.contains(forbidden),
            "миграция содержит разрушающую операцию {forbidden}"
        );
    }
    // Каждый CREATE INDEX — только IF NOT EXISTS, иначе повторный прогон падает.
    for (idx, _) in upper.match_indices("CREATE INDEX") {
        assert!(
            upper[idx..].starts_with("CREATE INDEX IF NOT EXISTS"),
            "индекс создаётся без IF NOT EXISTS"
        );
    }
}

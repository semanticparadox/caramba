//! Текстовые инварианты модели устройства (раунд 5, волна W2b, задача B2).
//!
//! Живой базы у тестов этого крейта нет (CI гоняет `cargo test` без Postgres),
//! поэтому проверки идут по исходникам — в том же стиле, что
//! `free_plan_grant_guard.rs` и `sql_dialect_guard.rs`.
//!
//! Каждый из этих инвариантов ломается молча: устройство продолжает работать,
//! список в кабинете продолжает рисоваться, и только человек однажды упирается
//! в лимит на собственном телефоне или не находит устройство, чтобы его
//! отвязать. Ни в логах, ни в ответах API такого отказа не видно.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn panel_src(relative: &str) -> String {
    repo_file(&format!("apps/caramba-panel/src/{relative}"))
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Вырезает кусок исходника между маркерами. Пропавший маркер — провал теста,
/// а не тихий пропуск проверки.
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

const MIGRATION: &str = "libs/caramba-db/migrations/20260911150000_device_identity.sql";

/// Миграция обязана оставаться аддитивной: релиз панели идёт с main, откат
/// делается только откатом кода, и DROP/RENAME в ней означал бы потерю данных
/// без возможности вернуться.
#[test]
fn the_device_migration_stays_additive() {
    let sql = repo_file(MIGRATION).to_uppercase();
    for forbidden in ["DROP TABLE", "DROP COLUMN", "RENAME COLUMN", "RENAME TO"] {
        assert!(
            !sql.contains(forbidden),
            "миграция устройств перестала быть аддитивной: найдено `{forbidden}`"
        );
    }
    for required in [
        "ADD COLUMN IF NOT EXISTS USER_ID",
        "ADD COLUMN IF NOT EXISTS CLIENT_DEVICE_ID",
        "ADD COLUMN IF NOT EXISTS PLATFORM",
    ] {
        assert!(
            sql.contains(required),
            "в миграции устройств пропало `{required}`"
        );
    }
}

/// Владельца лизы проставляет база. Лизу двигает не только панель: триггер
/// переноса из миграции 20260911140000 меняет subscription_id при смене тарифа,
/// и без этого триггера user_id остался бы от прежнего владельца строки.
#[test]
fn the_lease_owner_is_filled_by_the_database() {
    let sql = repo_file(MIGRATION);
    assert!(
        sql.contains("CREATE TRIGGER trg_device_leases_fill_user"),
        "исчез триггер, проставляющий владельца лизы"
    );
    assert!(
        squash(&sql)
            .contains("BEFORE INSERT OR UPDATE OF subscription_id ON subscription_device_leases"),
        "триггер владельца перестал срабатывать на переносе лизы между подписками"
    );
}

/// Индекс по (user_id, client_device_id) НЕ уникальный — намеренно. Уникальность
/// уронила бы перенос лиз при смене тарифа: он дедуплицирует строки по
/// device_fingerprint и про client_device_id не знает, поэтому две легаси-лизы
/// одного устройства столкнулись бы прямо посреди выдачи подписки.
#[test]
fn the_device_index_is_not_unique() {
    let sql = repo_file(MIGRATION);
    let index = between(
        &sql,
        "CREATE INDEX IF NOT EXISTS idx_subscription_device_leases_user_device",
        ";",
    );
    assert!(
        !index.to_uppercase().contains("UNIQUE"),
        "индекс устройств стал уникальным — перенос лиз при смене тарифа будет падать"
    );
}

/// Отпечаток устройства не имеет права зависеть от строки подписки: именно эта
/// зависимость обнуляла все привязки человека при каждой смене тарифа.
#[test]
fn the_fingerprint_does_not_depend_on_the_subscription() {
    let service = panel_src("services/subscription_service.rs");
    let body = between(
        &service,
        "pub fn device_fingerprint_for(",
        "hex::encode(hasher.finalize())",
    );
    assert!(
        !body.contains("subscription_id"),
        "отпечаток устройства снова считается от подписки — привязки будут слетать при смене тарифа"
    );
    assert!(
        body.contains("client_device_id") && body.contains("user_id"),
        "отпечаток устройства перестал опираться на идентификатор установки и владельца"
    );
}

/// Лимит и список устройств считаются по аккаунту. Счёт по подписке означает,
/// что устройства, оставшиеся на вытесненной строке, не видны ни в счётчике, ни
/// в кабинете — и отвязать их нечем.
#[test]
fn devices_are_counted_per_account() {
    let service = panel_src("services/subscription_service.rs");

    let counter = squash(between(
        &service,
        "async fn count_user_devices",
        "fetch_one(&self.pool)",
    ));
    assert!(
        counter.contains("WHERE user_id = $1"),
        "счётчик устройств вернулся к подписке вместо аккаунта: {counter}"
    );

    let list = squash(between(
        &service,
        "pub async fn list_user_devices",
        "fetch_all(&self.pool)",
    ));
    assert!(
        list.contains("WHERE sdl.user_id = $1"),
        "список устройств кабинета снова читается по подписке: {list}"
    );
}

/// Гейт подключения и запись лизы обязаны считать по одному ключу. Пока гейт
/// сравнивал адреса, а лиза писалась по отпечатку, лимит работал наоборот: пять
/// телефонов за одним NAT проходили насквозь, а один телефон при переключении
/// Wi-Fi → LTE упирался в лимит на самом себе.
#[test]
fn the_device_gate_and_the_lease_share_one_key() {
    let handler = panel_src("subscription.rs");
    let gate = between(
        &handler,
        "// 3.5 Лимит устройств.",
        "// 4. Update access tracking",
    );
    assert!(
        gate.contains("check_device_admission"),
        "гейт устройств перестал ходить через общий метод учёта"
    );
    assert!(
        !gate.contains("rec.client_ip"),
        "гейт устройств снова сравнивает адреса — лимит будет пробиваться любым NAT"
    );
}

/// Одно окно свежести на всю панель. Их было два (15 минут у гейта, час у
/// уборщика), и устройство, которым не пользовались вечер, наутро заводилось
/// заново, съедая слот лимита.
#[test]
fn there_is_exactly_one_freshness_window() {
    let service = panel_src("services/subscription_service.rs");
    assert!(
        service.contains("pub const DEVICE_LEASE_TTL_DAYS"),
        "исчезло единое окно свежести привязки устройства"
    );

    let cleanup = between(
        &service,
        "pub async fn cleanup_old_ip_tracking",
        "Ok(affected)",
    );
    assert!(
        cleanup.contains("DEVICE_LEASE_TTL_DAYS"),
        "уборщик лиз снова живёт по своему сроку, а не по общему окну"
    );
}

/// Мини-апп больше не ходит за устройствами по подписке: при смене тарифа часть
/// устройств оказывалась на прежней строке и была недоступна из кабинета вообще,
/// включая отвязку.
#[test]
fn the_miniapp_asks_for_devices_per_account() {
    let page = repo_file("apps/caramba-app/src/exa/pages/Devices.tsx");
    assert!(
        !page.contains("/api/client/subscription/"),
        "экран устройств мини-аппа снова ходит по подписке"
    );
    assert!(
        page.contains("'/api/client/devices'"),
        "экран устройств мини-аппа перестал запрашивать устройства аккаунта"
    );

    let api = panel_src("api/client.rs");
    for route in [
        "\"/devices\"",
        "\"/devices/{device_id}\"",
        "\"/devices/kill-all\"",
    ] {
        assert!(
            api.contains(route),
            "из api/client.rs пропал маршрут устройств аккаунта {route}"
        );
    }
}

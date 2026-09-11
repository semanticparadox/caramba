//! Инварианты состояния «прочитано» у тикетов (раунд 6, зона C).
//!
//! Живой базы у тестов этого крейта нет (CI гоняет `cargo test` без Postgres),
//! поэтому проверки идут по исходникам — как в `device_identity_guard.rs`.
//!
//! Что ломается молча, если эти инварианты уйдут: бейдж «новое» на тикете
//! перестаёт гаснуть от простого прочтения ответа, уведомление о новом ответе
//! теряет id тикета и тап по нему ведёт в никуда, а миграция с DROP/RENAME
//! стирает данные при релизе с main без пути назад.

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

const MIGRATION: &str = "libs/caramba-db/migrations/20260911180000_ticket_read_state.sql";

/// Миграция аддитивна: одна колонка `user_last_read_at` с IF NOT EXISTS,
/// без DROP/RENAME и без правки существующих строк.
#[test]
fn the_read_state_migration_stays_additive() {
    let sql = repo_file(MIGRATION).to_uppercase();
    for forbidden in [
        "DROP TABLE",
        "DROP COLUMN",
        "RENAME COLUMN",
        "RENAME TO",
        "UPDATE TICKETS",
    ] {
        assert!(
            !sql.contains(forbidden),
            "миграция read-state перестала быть аддитивной: найдено `{forbidden}`"
        );
    }
    assert!(
        squash(&sql).contains("ALTER TABLE TICKETS ADD COLUMN IF NOT EXISTS USER_LAST_READ_AT"),
        "миграция обязана добавлять tickets.user_last_read_at через ADD COLUMN IF NOT EXISTS"
    );
}

/// Счётчик непрочитанного в `list_user_tickets` обязан учитывать отметку
/// прочтения владельцем, а не только его последнее сообщение — иначе «новое»
/// гаснет лишь после собственного ответа.
#[test]
fn unread_for_user_respects_the_read_marker() {
    let src = panel_src("services/tickets_service.rs");
    let list = between(&src, "pub async fn list_user_tickets", "AS unread_for_user");
    let flat = squash(list);
    assert!(
        flat.contains("t.user_last_read_at"),
        "unread_for_user в list_user_tickets не смотрит на tickets.user_last_read_at"
    );
    assert!(
        flat.contains("GREATEST("),
        "unread_for_user обязан брать более позднюю из отметок: прочтение и последнее сообщение пользователя"
    );
    assert!(
        flat.contains("tm2.sender_role = 'user'"),
        "собственное сообщение пользователя больше не считается признаком прочтения"
    );
}

/// Открытие тикета владельцем (`get_ticket` без is_admin) ставит отметку;
/// админские пути (бот, модератор, админка) её не трогают.
#[test]
fn owner_read_marks_the_ticket_and_admin_read_does_not() {
    let src = panel_src("services/tickets_service.rs");
    let get = between(
        &src,
        "pub async fn get_ticket",
        "pub async fn mark_read_for_user",
    );
    let flat = squash(get);
    assert!(
        flat.contains("if !is_admin") && flat.contains("mark_read_for_user(ticket_id, uid)"),
        "get_ticket обязан вызывать mark_read_for_user только для не-админа"
    );
    let mark = between(&src, "pub async fn mark_read_for_user", "rows_affected()");
    let mark_flat = squash(mark);
    assert!(
        mark_flat.contains("SET user_last_read_at = NOW()")
            && mark_flat.contains("WHERE id = $1 AND user_id = $2"),
        "mark_read_for_user обязан обновлять только тикет владельца"
    );
    assert!(
        !mark_flat.contains("updated_at"),
        "прочтение не должно двигать updated_at: это ломает сортировку и автозакрытие"
    );

    // Админские вызовы сервиса идут с is_admin = true — отметка не ставится.
    for (file, marker) in [
        ("handlers/api/bot.rs", "get_ticket(ticket_id, true, None)"),
        (
            "handlers/api/moderator.rs",
            "get_ticket(ticket_id, true, None)",
        ),
    ] {
        assert!(
            panel_src(file).contains(marker),
            "{file}: админский get_ticket должен идти с is_admin = true"
        );
    }
}

/// API приложения: уведомление несёт `ticket_id`/`payload`, сводка тикета —
/// `has_unread`, детали — `category`. Без этого Flutter-клиент не может ни
/// открыть тикет по тапу на уведомление, ни погасить бейдж.
#[test]
fn app_api_exposes_ticket_navigation_and_read_state() {
    let src = panel_src("api/v2/app_support.rs");

    let notif = between(&src, "struct AppNotification {", "\n}");
    let notif_flat = squash(notif);
    assert!(
        notif_flat.contains("payload: Option<serde_json::Value>")
            && notif_flat.contains("ticket_id: Option<i64>"),
        "AppNotification обязан отдавать payload и ticket_id"
    );
    assert!(
        src.contains("ticket_id: ticket_id_from_payload(n.payload_json.as_ref())"),
        "list_notifications не заполняет ticket_id из payload_json"
    );

    let summary = between(&src, "struct AppTicketSummary {", "\n}");
    let summary_flat = squash(summary);
    for field in [
        "has_unread: bool",
        "category: String",
        "unread_for_user: i64",
    ] {
        assert!(
            summary_flat.contains(field),
            "AppTicketSummary потерял поле `{field}`"
        );
    }
    assert!(
        src.contains("has_unread: t.unread_for_user > 0"),
        "has_unread обязан выводиться из unread_for_user"
    );

    let detail = between(&src, "struct AppTicketDetail {", "\n}");
    assert!(
        squash(detail).contains("category: String"),
        "AppTicketDetail обязан отдавать category"
    );
}

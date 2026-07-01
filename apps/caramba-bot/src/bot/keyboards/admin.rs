//! Клавиатуры для административного интерфейса.
//!
//! Callback-data конвенция: `adm:<domain>:<action>[:<id>[:<param>]]`
//! Примеры:
//!   adm:menu                    — главное меню /admin
//!   adm:tickets:list:0:all      — список тикетов, страница 0, фильтр "all"
//!   adm:ticket:7                — детали тикета #7
//!   adm:reply:7                 — начать ввод ответа к тикету #7
//!   adm:assign:7                — назначить тикет #7 на себя
//!   adm:status:7                — меню смены статуса тикета #7
//!   adm:setstatus:7:resolved    — установить статус resolved для тикета #7
//!   adm:bcast:start             — начать broadcast
//!   adm:bcast:seg:<segment>     — выбор сегмента (шаг 1)
//!   adm:bcast:cat:<category>    — выбор категории (шаг 2)
//!   adm:bcast:sev:<severity>    — выбор уровня (шаг 3)
//!   adm:bcast:confirm           — подтверждение отправки
//!   adm:bcast:cancel            — отмена broadcast

use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

// ============================================================================
// Главное меню /admin
// ============================================================================

/// Построить главное меню с количеством открытых тикетов.
pub fn admin_main_menu(open_tickets: usize) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            format!("Тикеты ({} открытых)", open_tickets),
            "adm:tickets:list:0:open",
        )],
        vec![InlineKeyboardButton::callback(
            "Broadcast уведомление",
            "adm:bcast:start",
        )],
        vec![InlineKeyboardButton::callback(
            "Статистика",
            "adm:stats",
        )],
        vec![InlineKeyboardButton::callback(
            "Бренд",
            "adm:brand:menu",
        )],
        vec![InlineKeyboardButton::callback(
            "Модерация",
            "adm:moderation",
        )],
    ])
}

// ============================================================================
// Бренд — меню и поля
// ============================================================================

/// Меню брендинга. Кнопка enabled показывает текущее состояние и
/// переключает его. Остальные кнопки запускают ввод значения поля.
/// Callback-конвенция: `adm:brand:<action>[:<field>]`.
pub fn brand_menu_keyboard(enabled: bool) -> InlineKeyboardMarkup {
    let toggle_label = if enabled {
        "Брендинг: вкл (выключить)"
    } else {
        "Брендинг: выкл (включить)"
    };
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            toggle_label,
            "adm:brand:toggle",
        )],
        vec![InlineKeyboardButton::callback(
            "Название",
            "adm:brand:set:name",
        )],
        vec![InlineKeyboardButton::callback(
            "Логотип (URL)",
            "adm:brand:set:logo",
        )],
        vec![InlineKeyboardButton::callback(
            "Акцент (HEX)",
            "adm:brand:set:accent",
        )],
        vec![InlineKeyboardButton::callback(
            "Поддержка (URL)",
            "adm:brand:set:support",
        )],
        vec![InlineKeyboardButton::callback(
            "Бот (URL)",
            "adm:brand:set:boturl",
        )],
        vec![InlineKeyboardButton::callback(
            "В меню",
            "adm:menu",
        )],
    ])
}

/// Клавиатура под подсказкой ввода значения поля: только возврат в
/// меню бренда (он же отменяет ввод).
pub fn brand_field_back_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "Отмена",
        "adm:brand:menu",
    )]])
}

// ============================================================================
// Список тикетов
// ============================================================================

pub fn tickets_filter_row(current_filter: &str) -> Vec<InlineKeyboardButton> {
    let filters = [
        ("Все", "all"),
        ("Открытые", "open"),
        ("В работе", "in_progress"),
        ("Awaiting", "awaiting_user"),
        ("Закрытые", "closed"),
    ];
    filters
        .iter()
        .map(|(label, val)| {
            let display = if *val == current_filter {
                format!("• {}", label)
            } else {
                label.to_string()
            };
            InlineKeyboardButton::callback(display, format!("adm:tickets:list:0:{}", val))
        })
        .collect()
}

/// Emoji по категории тикета для отображения в списке.
pub fn category_emoji(cat: &str) -> &'static str {
    match cat {
        "billing" => "💳",
        "connection" => "🔌",
        "device" => "📱",
        "feature_request" => "💡",
        _ => "📋",
    }
}

/// Построить клавиатуру списка тикетов.
pub fn ticket_list_keyboard(
    ticket_ids: &[(i64, &str, &str)], // (id, category, subject)
    page: usize,
    total_pages: usize,
    current_filter: &str,
) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    // Строки тикетов
    for (id, cat, subject) in ticket_ids {
        let emoji = category_emoji(cat);
        // Char-count truncation, not byte-slice — Cyrillic/emoji subjects
        // would panic on a non-char-boundary slice.
        let short_subject = if subject.chars().count() > 40 {
            let prefix: String = subject.chars().take(40).collect();
            format!("{}…", prefix)
        } else {
            subject.to_string()
        };
        rows.push(vec![InlineKeyboardButton::callback(
            format!("#{} {} {}", id, emoji, short_subject),
            format!("adm:ticket:{}", id),
        )]);
    }

    // Фильтры
    rows.push(tickets_filter_row(current_filter));

    // Навигация
    let mut nav_row: Vec<InlineKeyboardButton> = Vec::new();
    if page > 0 {
        nav_row.push(InlineKeyboardButton::callback(
            "Назад",
            format!("adm:tickets:list:{}:{}", page - 1, current_filter),
        ));
    }
    if total_pages > 1 {
        nav_row.push(InlineKeyboardButton::callback(
            format!("{}/{}", page + 1, total_pages),
            "adm:noop",
        ));
    }
    if page + 1 < total_pages {
        nav_row.push(InlineKeyboardButton::callback(
            "Далее",
            format!("adm:tickets:list:{}:{}", page + 1, current_filter),
        ));
    }
    if !nav_row.is_empty() {
        rows.push(nav_row);
    }

    rows.push(vec![InlineKeyboardButton::callback(
        "Закрыть",
        "adm:close",
    )]);

    InlineKeyboardMarkup::new(rows)
}

// ============================================================================
// Детали тикета
// ============================================================================

pub fn ticket_detail_keyboard(ticket_id: i64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback(
                "Ответить",
                format!("adm:reply:{}", ticket_id),
            ),
            InlineKeyboardButton::callback(
                "Взять",
                format!("adm:assign:{}", ticket_id),
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "Статус",
            format!("adm:status:{}", ticket_id),
        )],
        vec![InlineKeyboardButton::callback(
            "К списку",
            "adm:tickets:list:0:open",
        )],
    ])
}

// ============================================================================
// Меню смены статуса
// ============================================================================

pub fn ticket_status_keyboard(ticket_id: i64) -> InlineKeyboardMarkup {
    let statuses = [
        ("В работе", "in_progress"),
        ("Ожидание ответа", "awaiting_user"),
        ("Решён", "resolved"),
        ("Закрыт", "closed"),
        ("Открыт", "open"),
    ];
    let mut rows: Vec<Vec<InlineKeyboardButton>> = statuses
        .iter()
        .map(|(label, val)| {
            vec![InlineKeyboardButton::callback(
                *label,
                format!("adm:setstatus:{}:{}", ticket_id, val),
            )]
        })
        .collect();
    rows.push(vec![InlineKeyboardButton::callback(
        "Назад",
        format!("adm:ticket:{}", ticket_id),
    )]);
    InlineKeyboardMarkup::new(rows)
}

// ============================================================================
// Broadcast — пошаговые клавиатуры
// ============================================================================

pub fn broadcast_segment_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            "Все пользователи",
            "adm:bcast:seg:all",
        )],
        vec![InlineKeyboardButton::callback(
            "Активные подписки",
            "adm:bcast:seg:active_subs",
        )],
        vec![InlineKeyboardButton::callback(
            "Trial",
            "adm:bcast:seg:trial",
        )],
        vec![InlineKeyboardButton::callback(
            "Отменить",
            "adm:bcast:cancel",
        )],
    ])
}

pub fn broadcast_category_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Биллинг", "adm:bcast:cat:billing"),
            InlineKeyboardButton::callback("Подключение", "adm:bcast:cat:connection"),
        ],
        vec![
            InlineKeyboardButton::callback("Устройства", "adm:bcast:cat:device"),
            InlineKeyboardButton::callback("Фичи", "adm:bcast:cat:feature_request"),
        ],
        vec![
            InlineKeyboardButton::callback("Обслуживание", "adm:bcast:cat:maintenance"),
            InlineKeyboardButton::callback("Прочее", "adm:bcast:cat:other"),
        ],
        vec![InlineKeyboardButton::callback(
            "Отменить",
            "adm:bcast:cancel",
        )],
    ])
}

pub fn broadcast_severity_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Info", "adm:bcast:sev:info"),
            InlineKeyboardButton::callback("Warning", "adm:bcast:sev:warning"),
            InlineKeyboardButton::callback("Error", "adm:bcast:sev:error"),
        ],
        vec![InlineKeyboardButton::callback(
            "Отменить",
            "adm:bcast:cancel",
        )],
    ])
}

pub fn broadcast_confirm_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("Отправить", "adm:bcast:confirm"),
        InlineKeyboardButton::callback("Отменить", "adm:bcast:cancel"),
    ]])
}

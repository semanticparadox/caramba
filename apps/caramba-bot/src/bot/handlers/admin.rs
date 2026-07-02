//! Административный интерфейс бота.
//!
//! Доступ строго ограничен: каждый хендлер начинается с проверки `is_admin()`.
//! Не-администраторы получают тихий отказ или никакой реакции.
//!
//! FSM-состояния (добавлены к существующей логике бота):
//!   AdminReplyTo { ticket_id }  — ожидаем текст ответа в тикет
//!   AdminBcastTitle { ... }     — ожидаем заголовок broadcast
//!   AdminBcastBody { ... }      — ожидаем тело broadcast
//!   AdminBcastConfirm { ... }   — показан preview, ожидаем кнопку

use crate::bot::keyboards::admin::{
    admin_main_menu, broadcast_confirm_keyboard, ticket_detail_keyboard, ticket_list_keyboard,
};
use crate::bot::translations::t;
use crate::services::admin_service::BroadcastRequest;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// ============================================================================
// FSM state definitions
// ============================================================================

/// Все возможные FSM-состояния для администратора.
/// Хранятся в `AppState.admin_fsm` (per tg_id, in-memory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminFsmState {
    /// Ожидаем текст ответа на тикет.
    ReplyTo { ticket_id: i64 },

    /// Ожидаем заголовок broadcast-уведомления.
    BcastAwaitTitle {
        segment: String,
        category: String,
        severity: String,
    },

    /// Ожидаем тело broadcast-уведомления.
    BcastAwaitBody {
        segment: String,
        category: String,
        severity: String,
        title: String,
    },

    /// Broadcast полностью заполнен, показан preview — ожидаем Confirm/Cancel.
    BcastReady {
        segment: String,
        category: String,
        severity: String,
        title: String,
        body: String,
    },

    /// Ожидаем ввод значения для одного из brand_*-ключей.
    /// `field` — короткий идентификатор поля (см. brand::BrandField),
    /// по нему хендлер выбирает целевой settings-ключ и валидацию.
    BrandAwaitValue { field: String },
}

// ============================================================================
// FSM storage
// ============================================================================

/// Потокобезопасное хранилище FSM-состояний администраторов.
#[derive(Clone, Default)]
pub struct AdminFsmStorage {
    inner: Arc<RwLock<HashMap<i64, AdminFsmState>>>,
}

impl AdminFsmStorage {
    pub async fn get(&self, tg_id: i64) -> Option<AdminFsmState> {
        self.inner.read().await.get(&tg_id).cloned()
    }

    pub async fn set(&self, tg_id: i64, state: AdminFsmState) {
        self.inner.write().await.insert(tg_id, state);
    }

    pub async fn clear(&self, tg_id: i64) {
        self.inner.write().await.remove(&tg_id);
    }
}

// ============================================================================
// Label helpers
// ============================================================================

pub fn status_label(status: &str) -> &'static str {
    match status {
        "open" => "Открыт",
        "in_progress" => "В работе",
        "awaiting_user" => "Ожидание ответа",
        "resolved" => "Решён",
        "closed" => "Закрыт",
        _ => "Неизвестно",
    }
}

pub fn category_label(cat: &str) -> &'static str {
    match cat {
        "billing" => "Биллинг",
        "connection" => "Подключение",
        "device" => "Устройство",
        "feature_request" => "Запрос фичи",
        "maintenance" => "Обслуживание",
        _ => "Прочее",
    }
}

pub fn severity_label(sev: &str) -> &'static str {
    match sev {
        "info" => "Info",
        "warning" => "Warning",
        "error" => "Error",
        _ => "Info",
    }
}

pub fn segment_label(seg: &str) -> &'static str {
    match seg {
        "all" => "Все пользователи",
        "active_subs" => "Активные подписки",
        "trial" => "Trial",
        "open" => "Открытые",
        "in_progress" => "В работе",
        "awaiting_user" => "Awaiting user",
        "closed" => "Закрытые",
        _ => "Все",
    }
}

// ============================================================================
// /admin command handler
// ============================================================================

/// Обработчик команды /admin.
/// Для не-администраторов: полное молчание — команда не раскрывается.
pub async fn handle_admin_command(
    bot: Bot,
    msg: Message,
    state: AppState,
) -> Result<(), teloxide::RequestError> {
    let tg_id = msg.chat.id.0;

    if !state.admin_service.is_admin(tg_id).await {
        // Тихий отказ — не отвечаем, не раскрываем команду
        return Ok(());
    }

    info!(tg_id, "Admin opened /admin menu");

    // Получаем количество открытых тикетов для счётчика в меню
    let open_count = state
        .admin_service
        .list_tickets(Some("open"), 100, 0)
        .await
        .map(|v| v.len())
        .unwrap_or(0);

    let _ = bot
        .send_message(msg.chat.id, t(None, "admin.menu.title"))
        .parse_mode(ParseMode::Html)
        .reply_markup(admin_main_menu(open_count))
        .await;

    Ok(())
}

// ============================================================================
// Stats sub-handler
// ============================================================================

pub async fn send_admin_stats(
    bot: &Bot,
    chat_id: ChatId,
    state: &AppState,
) -> Result<(), teloxide::RequestError> {
    let open_tickets = state
        .admin_service
        .list_tickets(Some("open"), 100, 0)
        .await
        .map(|v| v.len())
        .unwrap_or(0);

    match state.store_service.get_system_stats().await {
        Ok(s) => {
            let text = format!(
                "<b>Статистика системы</b>\n\n\
                Ноды: {} активных\n\
                Пользователей: {}\n\
                Активных подписок: {}\n\
                Открытых тикетов: {}\n\
                Доход: ${:.2}\n\
                Трафик (30д): {} GB",
                s.active_nodes,
                s.total_users,
                s.active_subs,
                open_tickets,
                s.total_revenue,
                s.traffic_30d_gb,
            );
            let _ = bot
                .send_message(chat_id, text)
                .parse_mode(ParseMode::Html)
                .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                    InlineKeyboardButton::callback("В меню", "adm:menu"),
                ]]))
                .await;
        }
        Err(e) => {
            error!(error = %e, "Admin stats failed");
            let _ = bot
                .send_message(chat_id, "Не удалось загрузить статистику.")
                .await;
        }
    }
    Ok(())
}

// ============================================================================
// Ticket list
// ============================================================================

const PAGE_SIZE: usize = 5;

pub async fn send_ticket_list(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: Option<MessageId>,
    state: &AppState,
    page: usize,
    filter: &str,
) -> Result<(), teloxide::RequestError> {
    let status_param = if filter == "all" { None } else { Some(filter) };

    let all_tickets = state
        .admin_service
        .list_tickets(status_param, 200, 0)
        .await
        .unwrap_or_default();

    if all_tickets.is_empty() {
        let text = t(None, "admin.tickets.empty");
        let back_kb = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "В меню",
            "adm:menu",
        )]]);
        if let Some(mid) = msg_id {
            let _ = bot
                .edit_message_text(chat_id, mid, text)
                .reply_markup(back_kb)
                .await;
        } else {
            let _ = bot.send_message(chat_id, text).reply_markup(back_kb).await;
        }
        return Ok(());
    }

    let total_pages = all_tickets.len().div_ceil(PAGE_SIZE);
    let page = page.min(total_pages.saturating_sub(1));
    let slice = &all_tickets[page * PAGE_SIZE..((page + 1) * PAGE_SIZE).min(all_tickets.len())];

    let ticket_data: Vec<(i64, &str, &str)> = slice
        .iter()
        .map(|tk| (tk.id, tk.category.as_str(), tk.subject.as_str()))
        .collect();

    let kb = ticket_list_keyboard(&ticket_data, page, total_pages, filter);

    let header = format!(
        "<b>Тикеты</b> [{}] — стр. {}/{}",
        segment_label(filter),
        page + 1,
        total_pages
    );

    if let Some(mid) = msg_id {
        let _ = bot
            .edit_message_text(chat_id, mid, &header)
            .parse_mode(ParseMode::Html)
            .reply_markup(kb)
            .await;
    } else {
        let _ = bot
            .send_message(chat_id, &header)
            .parse_mode(ParseMode::Html)
            .reply_markup(kb)
            .await;
    }
    Ok(())
}

// ============================================================================
// Ticket detail
// ============================================================================

pub async fn send_ticket_detail(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: Option<MessageId>,
    state: &AppState,
    ticket_id: i64,
) -> Result<(), teloxide::RequestError> {
    let detail = match state.admin_service.get_ticket(ticket_id).await {
        Ok(d) => d,
        Err(e) => {
            error!(ticket_id, error = %e, "Failed to fetch ticket detail");
            let _ = bot
                .send_message(
                    chat_id,
                    format!("Не удалось загрузить тикет #{}", ticket_id),
                )
                .await;
            return Ok(());
        }
    };

    let tk = &detail.ticket;
    let u = &detail.user;

    let assignee_str = tk
        .assignee_tg_id
        .map(|id| format!("tg:{}", id))
        .unwrap_or_else(|| "Не назначен".to_string());

    let user_str = match &u.username {
        Some(un) => format!("@{} ({})", un, u.tg_id),
        None => format!("tg:{}", u.tg_id),
    };

    let mut text = format!(
        "<b>Тикет #{}</b>\n\
        Тема: {}\n\
        Статус: {}\n\
        Категория: {}\n\
        Пользователь: {}\n\
        Назначен: {}\n\n\
        <b>Последние сообщения:</b>\n",
        tk.id,
        tk.subject,
        status_label(&tk.status),
        category_label(&tk.category),
        user_str,
        assignee_str,
    );

    // Последние 5 сообщений (в хронологическом порядке)
    let msgs: Vec<_> = detail.messages.iter().rev().take(5).collect();
    for m in msgs.into_iter().rev() {
        let who = if m.sender_role == "admin" {
            "Admin"
        } else {
            "User"
        };
        // Char-count truncation, not byte-slice — Cyrillic/emoji bodies
        // would panic on a non-char-boundary slice.
        let body_short = if m.body.chars().count() > 200 {
            let prefix: String = m.body.chars().take(200).collect();
            format!("{}…", prefix)
        } else {
            m.body.clone()
        };
        text.push_str(&format!(
            "[{}] {}: {}\n",
            who,
            m.created_at.format("%m-%d %H:%M"),
            body_short
        ));
    }

    let kb = ticket_detail_keyboard(tk.id);

    if let Some(mid) = msg_id {
        let _ = bot
            .edit_message_text(chat_id, mid, &text)
            .parse_mode(ParseMode::Html)
            .reply_markup(kb)
            .await;
    } else {
        let _ = bot
            .send_message(chat_id, &text)
            .parse_mode(ParseMode::Html)
            .reply_markup(kb)
            .await;
    }
    Ok(())
}

// ============================================================================
// Broadcast summary formatter
// ============================================================================

pub fn build_broadcast_summary(
    segment: &str,
    category: &str,
    severity: &str,
    title: &str,
    body: &str,
) -> String {
    format!(
        "<b>Подтверждение broadcast</b>\n\n\
        Сегмент: {}\n\
        Категория: {}\n\
        Уровень: {}\n\
        Заголовок: <b>{}</b>\n\
        Текст: {}",
        segment_label(segment),
        category_label(category),
        severity_label(severity),
        title,
        body,
    )
}

// ============================================================================
// FSM text input handler
// ============================================================================

/// Вызывается из message_handler для обработки текстовых вводов в FSM-состояниях.
/// Возвращает `true` если сообщение обработано (не нужно продолжать обработку).
pub async fn handle_admin_text(
    bot: &Bot,
    msg: &Message,
    tg_id: i64,
    text: &str,
    state: &AppState,
) -> bool {
    let fsm_state = state.admin_fsm.get(tg_id).await;

    match fsm_state {
        // -----------------------------------------------------------------------
        // Ответ на тикет
        // -----------------------------------------------------------------------
        Some(AdminFsmState::ReplyTo { ticket_id }) => {
            match state
                .admin_service
                .reply_to_ticket(ticket_id, tg_id, text)
                .await
            {
                Ok(_) => {
                    state.admin_fsm.clear(tg_id).await;
                    let _ = bot
                        .send_message(msg.chat.id, t(None, "admin.reply.sent"))
                        .await;
                }
                Err(e) => {
                    error!(ticket_id, tg_id, error = %e, "Failed to post admin reply");
                    state.admin_fsm.clear(tg_id).await;
                    let _ = bot
                        .send_message(msg.chat.id, "Не удалось отправить ответ. Попробуйте снова.")
                        .await;
                }
            }
            true
        }

        // -----------------------------------------------------------------------
        // Broadcast: ввод заголовка
        // -----------------------------------------------------------------------
        Some(AdminFsmState::BcastAwaitTitle {
            segment,
            category,
            severity,
        }) => {
            // Category & severity are populated by inline-keyboard callbacks
            // before the title step. If the admin types a message before
            // pressing those buttons, those fields are still empty — accepting
            // the text here would lead to a broadcast with blank category/
            // severity that breaks Mini App filtering. Re-prompt instead.
            if category.trim().is_empty() || severity.trim().is_empty() {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        t(None, "admin.broadcast.step_keyboard_required"),
                    )
                    .await;
                return true;
            }

            state
                .admin_fsm
                .set(
                    tg_id,
                    AdminFsmState::BcastAwaitBody {
                        segment,
                        category,
                        severity,
                        title: text.to_string(),
                    },
                )
                .await;
            let _ = bot
                .send_message(msg.chat.id, t(None, "admin.broadcast.step_body"))
                .await;
            true
        }

        // -----------------------------------------------------------------------
        // Broadcast: ввод тела → показываем подтверждение
        // -----------------------------------------------------------------------
        Some(AdminFsmState::BcastAwaitBody {
            segment,
            category,
            severity,
            title,
        }) => {
            let summary = build_broadcast_summary(&segment, &category, &severity, &title, text);

            state
                .admin_fsm
                .set(
                    tg_id,
                    AdminFsmState::BcastReady {
                        segment,
                        category,
                        severity,
                        title,
                        body: text.to_string(),
                    },
                )
                .await;

            let _ = bot
                .send_message(msg.chat.id, summary)
                .parse_mode(ParseMode::Html)
                .reply_markup(broadcast_confirm_keyboard())
                .await;
            true
        }

        // Уже в состоянии Ready — ожидаем кнопку, игнорируем текст
        Some(AdminFsmState::BcastReady { .. }) => true,

        // -----------------------------------------------------------------------
        // Бренд: ввод значения поля
        // -----------------------------------------------------------------------
        Some(AdminFsmState::BrandAwaitValue { field }) => {
            crate::bot::handlers::brand::handle_brand_value(
                bot,
                msg.chat.id,
                tg_id,
                &field,
                text,
                state,
            )
            .await
        }

        None => false,
    }
}

// ============================================================================
// Broadcast confirm handler (вызывается из callback_handler)
// ============================================================================

pub async fn handle_broadcast_confirm(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: Option<MessageId>,
    tg_id: i64,
    state: &AppState,
) -> Result<(), teloxide::RequestError> {
    let fsm_state = state.admin_fsm.get(tg_id).await;

    match fsm_state {
        Some(AdminFsmState::BcastReady {
            segment,
            category,
            severity,
            title,
            body,
        }) => {
            state.admin_fsm.clear(tg_id).await;

            let req = BroadcastRequest {
                category: category.clone(),
                severity: severity.clone(),
                title: title.clone(),
                body: body.clone(),
                payload: None,
                segment: if segment == "all" {
                    None
                } else {
                    Some(segment.clone())
                },
            };

            match state.admin_service.send_broadcast(req).await {
                Ok(queued) => {
                    let text = format!("Отправлено {} пользователям.", queued);
                    if let Some(mid) = msg_id {
                        let _ = bot
                            .edit_message_text(chat_id, mid, text)
                            .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback("В меню", "adm:menu"),
                            ]]))
                            .await;
                    } else {
                        let _ = bot.send_message(chat_id, text).await;
                    }
                }
                Err(e) => {
                    error!(tg_id, error = %e, "Broadcast send failed");
                    let _ = bot
                        .send_message(chat_id, "Ошибка отправки broadcast. Проверьте логи.")
                        .await;
                }
            }
        }
        _ => {
            warn!(tg_id, "broadcast confirm without BcastReady state");
            let _ = bot
                .send_message(chat_id, "Нет активного broadcast. Начните заново.")
                .await;
        }
    }
    Ok(())
}

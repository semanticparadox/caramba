//! Редактор шаблонов системных уведомлений.
//!
//! Пишет в `notification_templates` (миграция 20260831140000) и после каждой
//! записи перезагружает кэш сервиса — иначе оператор сохранил бы текст и не
//! увидел его до перезапуска панели.
//!
//! Валидация здесь не формальность. У этих девяти сообщений один путь наружу и
//! ни одного человека между ошибкой и адресатом: `{9}` уедет в чат дословно,
//! незакрытый `<b>` заставит Telegram отвергнуть сообщение целиком, а подпись
//! длиннее 1024 символов при заданном медиа вернёт ошибку уже из `send_photo`.
//! Всё это ловится на сохранении, где оно ещё сообщение об ошибке.

use axum::{
    Form,
    extract::{Path, State},
    response::{IntoResponse, Redirect},
};
use serde::Deserialize;
use tracing::error;

use crate::AppState;
use crate::bot::translations::Lang;
use crate::services::notification_templates::{ButtonTarget, deep_link, event};

#[derive(Deserialize)]
pub struct TemplateForm {
    pub text: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub parse_mode: Option<String>,
    pub media_type: Option<String>,
    pub media_url: Option<String>,
    pub button_text: Option<String>,
    /// `billing` | `plans` | `subscription` | `custom` — что выбрано в списке целей.
    pub button_target: Option<String>,
    /// Заполняется только при `button_target = custom`.
    pub button_url: Option<String>,
    pub disable_link_preview: Option<String>,
}

fn clean(v: Option<String>) -> Option<String> {
    v.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
}

fn parse_lang(raw: &str) -> Option<Lang> {
    match raw {
        "ru" => Some(Lang::Ru),
        "en" => Some(Lang::En),
        _ => None,
    }
}

fn target_from(raw: &str) -> Option<ButtonTarget> {
    ButtonTarget::ALL.iter().copied().find(|t| t.slug() == raw)
}

/// Грубая проверка парности тегов для HTML parse mode.
///
/// Telegram принимает узкий набор тегов и отвергает СООБЩЕНИЕ ЦЕЛИКОМ на
/// незакрытом. Полноценный парсер здесь не нужен и был бы обманом строгости:
/// достаточно поймать самую частую ошибку живого редактирования — открыл и
/// забыл закрыть.
fn html_tags_balanced(text: &str) -> Result<(), String> {
    let mut stack: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else {
            return Err("Незакрытая угловая скобка «<»".to_string());
        };
        let tag = &rest[..close];
        rest = &rest[close + 1..];
        let name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if tag.starts_with('/') {
            match stack.pop() {
                Some(open_name) if open_name == name => {}
                Some(open_name) => {
                    return Err(format!("Тег </{name}> закрывает <{open_name}>"));
                }
                None => return Err(format!("Тег </{name}> закрывает то, что не открыто")),
            }
        } else {
            stack.push(name);
        }
    }
    match stack.last() {
        Some(name) => Err(format!("Тег <{name}> не закрыт")),
        None => Ok(()),
    }
}

/// Сохранение одного шаблона: `POST /admin/notification-templates/{event}/{lang}`.
pub async fn save_template(
    State(state): State<AppState>,
    Path((event_key, lang_raw)): Path<(String, String)>,
    Form(form): Form<TemplateForm>,
) -> impl IntoResponse {
    let redirect = format!("{}/notifications", state.admin_path);

    let (Some(ev), Some(lang)) = (event(&event_key), parse_lang(&lang_raw)) else {
        return Redirect::to(&format!("{redirect}?tpl_error=unknown_event")).into_response();
    };

    let text = clean(form.text);
    let title = clean(form.title);
    let body = clean(form.body);
    let parse_mode = form.parse_mode.unwrap_or_else(|| "html".into());

    // Подстановки — по каждому редактируемому полю, а не только по тексту:
    // карточка в приложении собирается из тех же аргументов.
    for candidate in [text.as_deref(), title.as_deref(), body.as_deref()]
        .into_iter()
        .flatten()
    {
        if let Err(msg) = ev.validate_placeholders(candidate) {
            return Redirect::to(&format!(
                "{redirect}?tpl_error={}",
                urlencoding::encode(&msg)
            ))
            .into_response();
        }
        if parse_mode == "html"
            && let Err(msg) = html_tags_balanced(candidate)
        {
            return Redirect::to(&format!(
                "{redirect}?tpl_error={}",
                urlencoding::encode(&msg)
            ))
            .into_response();
        }
    }

    let media_type = form.media_type.unwrap_or_else(|| "none".into());
    let media_url = clean(form.media_url);

    // Telegram режет подпись к медиа на 1024 символах, и send_rich_notification
    // возвращает ошибку вместо отправки — уведомление потерялось бы молча.
    if media_type != "none"
        && let Some(t) = text.as_deref()
        && t.chars().count() > 1024
    {
        return Redirect::to(&format!(
            "{redirect}?tpl_error={}",
            urlencoding::encode("С медиа текст не может быть длиннее 1024 символов")
        ))
        .into_response();
    }

    // Кнопка: либо готовая цель из списка, либо свой абсолютный URL.
    let buttons_json = match (clean(form.button_text), form.button_target.as_deref()) {
        (Some(label), Some("custom")) => match clean(form.button_url) {
            Some(url) if url::Url::parse(&url).is_ok() => {
                Some(serde_json::json!([{ "text": label, "url": url }]))
            }
            _ => {
                return Redirect::to(&format!(
                    "{redirect}?tpl_error={}",
                    urlencoding::encode("Ссылка кнопки должна быть абсолютной (https://…)")
                ))
                .into_response();
            }
        },
        (Some(label), Some(slug)) => {
            let bot = state.settings.get("bot_username").await;
            let short = state
                .settings
                .get_or_default("mini_app_short_name", "")
                .await;
            match target_from(slug).and_then(|t| deep_link(bot.as_deref(), &short, t)) {
                Some(url) => Some(serde_json::json!([{ "text": label, "url": url }])),
                None => {
                    return Redirect::to(&format!(
                        "{redirect}?tpl_error={}",
                        urlencoding::encode(
                            "Не из чего собрать ссылку: не заполнено имя бота в настройках"
                        )
                    ))
                    .into_response();
                }
            }
        }
        _ => None,
    };

    let result = sqlx::query(
        "INSERT INTO notification_templates \
         (event, lang, text, title, body, parse_mode, media_type, media_url, \
          buttons_json, disable_link_preview, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, CURRENT_TIMESTAMP) \
         ON CONFLICT (event, lang) DO UPDATE SET \
           text = EXCLUDED.text, title = EXCLUDED.title, body = EXCLUDED.body, \
           parse_mode = EXCLUDED.parse_mode, media_type = EXCLUDED.media_type, \
           media_url = EXCLUDED.media_url, buttons_json = EXCLUDED.buttons_json, \
           disable_link_preview = EXCLUDED.disable_link_preview, \
           updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&event_key)
    .bind(lang.as_str())
    .bind(&text)
    .bind(&title)
    .bind(&body)
    .bind(&parse_mode)
    .bind(&media_type)
    .bind(&media_url)
    .bind(&buttons_json)
    .bind(form.disable_link_preview.is_some())
    .execute(&state.pool)
    .await;

    if let Err(e) = result {
        error!(error = %e, event = %event_key, "notification template save failed");
        return Redirect::to(&format!("{redirect}?tpl_error=save_failed")).into_response();
    }

    if let Err(e) = state.notification_templates.reload_cache().await {
        error!(error = %e, "notification template cache reload failed");
    }

    Redirect::to(&format!("{redirect}?tpl_saved={event_key}")).into_response()
}

/// Возврат к встроенному тексту: `POST /admin/notification-templates/{event}/{lang}/reset`.
///
/// Именно DELETE, а не запись копии дефолта: строка-копия заморозила бы текст на
/// сегодняшней редакции и перестала бы получать правки, приезжающие с кодом.
pub async fn reset_template(
    State(state): State<AppState>,
    Path((event_key, lang_raw)): Path<(String, String)>,
) -> impl IntoResponse {
    let redirect = format!("{}/notifications", state.admin_path);
    let Some(lang) = parse_lang(&lang_raw) else {
        return Redirect::to(&format!("{redirect}?tpl_error=unknown_event")).into_response();
    };

    if let Err(e) =
        sqlx::query("DELETE FROM notification_templates WHERE event = $1 AND lang = $2")
            .bind(&event_key)
            .bind(lang.as_str())
            .execute(&state.pool)
            .await
    {
        error!(error = %e, event = %event_key, "notification template reset failed");
        return Redirect::to(&format!("{redirect}?tpl_error=save_failed")).into_response();
    }

    if let Err(e) = state.notification_templates.reload_cache().await {
        error!(error = %e, "notification template cache reload failed");
    }

    Redirect::to(&format!("{redirect}?tpl_reset={event_key}")).into_response()
}

/// Тестовая отправка себе: `POST /admin/notification-templates/{event}/{lang}/test`.
///
/// Единственный честный предпросмотр. Панель может показать текст, но не то, как
/// Telegram разберёт разметку, склеит кнопки и покажет медиа, — а отвергает
/// сообщение именно он.
pub async fn test_template(
    State(state): State<AppState>,
    Path((event_key, lang_raw)): Path<(String, String)>,
) -> impl IntoResponse {
    let redirect = format!("{}/notifications", state.admin_path);
    let (Some(ev), Some(lang)) = (event(&event_key), parse_lang(&lang_raw)) else {
        return Redirect::to(&format!("{redirect}?tpl_error=unknown_event")).into_response();
    };

    let Some(tg_id) = first_admin_tg_id(&state).await else {
        return Redirect::to(&format!(
            "{redirect}?tpl_error={}",
            urlencoding::encode(
                "Некуда слать тест: заполните «Telegram ID администраторов» в настройках"
            )
        ))
        .into_response();
    };

    // Примерные значения: тест должен показывать сообщение целиком, а не с
    // дырами на месте подстановок.
    let samples: Vec<String> = ev
        .args
        .iter()
        .map(|label| format!("‹{label}›"))
        .collect();
    let sample_refs: Vec<&str> = samples.iter().map(String::as_str).collect();

    let rendered = state
        .notification_templates
        .render_with(&state.settings, &event_key, lang, &sample_refs)
        .await;

    match state
        .bot_manager
        .send_rich_notification(tg_id, rendered.payload)
        .await
    {
        Ok(()) => Redirect::to(&format!("{redirect}?tpl_tested={event_key}")).into_response(),
        Err(e) => Redirect::to(&format!(
            "{redirect}?tpl_error={}",
            urlencoding::encode(&format!("Telegram отверг сообщение: {e}"))
        ))
        .into_response(),
    }
}

/// Telegram-id первого администратора из настроек.
///
/// В таблице `admins` его нет — учётная запись панели и аккаунт Telegram здесь
/// разные вещи. Адресаты админских сообщений живут в настройке
/// `admin_notification_tg_ids`, той же, что использует `notify_admins`; берём
/// первый разбираемый id, чтобы тест ушёл одному человеку, а не всей команде.
async fn first_admin_tg_id(state: &AppState) -> Option<i64> {
    state
        .settings
        .get_or_default("admin_notification_tg_ids", "")
        .await
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .next()
}

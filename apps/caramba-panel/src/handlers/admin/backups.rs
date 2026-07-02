/// Обработчики резервного копирования БД — административный раздел.
///
/// Маршруты (все защищены auth_middleware):
///   GET  {admin_path}/backups           — страница со списком резервных копий
///   POST {admin_path}/backups/create    — ручной запуск резервного копирования
///   GET  {admin_path}/backups/{file}    — скачать файл резервной копии
///   POST {admin_path}/backups/{file}/delete — удалить файл и вернуться к списку
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use tracing::error;

use super::auth::get_auth_user;
use crate::AppState;
use crate::services::backup_service::{self, BackupInfo};

// ────────────────────────────────────────────────────────────────────────────
// Шаблон
// ────────────────────────────────────────────────────────────────────────────

/// Строка таблицы резервных копий — готовая к отображению.
pub struct BackupRow {
    pub filename: String,
    pub created_at: String,
    pub size_human: String,
    pub age_hours: i64,
}

/// Статус последней резервной копии для индикаторного пилюли.
pub enum BackupStatusPill {
    /// Нет ни одной копии
    Never,
    /// Моложе 26 часов — зелёный
    Fresh,
    /// Старше 26 часов, но моложе 72 — жёлтый
    Stale,
    /// Старше 72 часов — красный
    Old,
}

impl BackupStatusPill {
    fn from_backups(backups: &[BackupInfo]) -> Self {
        match backups.first() {
            None => BackupStatusPill::Never,
            Some(b) => {
                let age_h = (chrono::Utc::now() - b.created_at).num_hours();
                if age_h < 26 {
                    BackupStatusPill::Fresh
                } else if age_h < 72 {
                    BackupStatusPill::Stale
                } else {
                    BackupStatusPill::Old
                }
            }
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            BackupStatusPill::Never | BackupStatusPill::Old => {
                "bg-rose-500/10 text-rose-400 border-rose-500/20"
            }
            BackupStatusPill::Stale => "bg-amber-500/10 text-amber-400 border-amber-500/20",
            BackupStatusPill::Fresh => "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BackupStatusPill::Never => "Never",
            BackupStatusPill::Fresh => "Fresh",
            BackupStatusPill::Stale => "Stale",
            BackupStatusPill::Old => "Old",
        }
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "backups.html")]
pub struct BackupsTemplate {
    pub is_auth: bool,
    pub username: String,
    pub admin_path: String,
    pub active_page: String,
    pub rows: Vec<BackupRow>,
    pub status_class: String,
    pub status_label: String,
    pub last_backup_ago: String,
    pub backup_count: usize,
    pub flash: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────
// Вспомогательные функции
// ────────────────────────────────────────────────────────────────────────────

fn format_age(h: i64) -> String {
    if h < 1 {
        "just now".to_string()
    } else if h < 24 {
        format!("{}h ago", h)
    } else {
        format!("{}d ago", h / 24)
    }
}

async fn render_page(state: &AppState, jar: &CookieJar, flash: Option<String>) -> Html<String> {
    let username = get_auth_user(state, jar)
        .await
        .unwrap_or_else(|| "Admin".to_string());

    let backups = backup_service::list_backups().await.unwrap_or_default();

    let pill = BackupStatusPill::from_backups(&backups);
    let last_ago = match backups.first() {
        None => "Never".to_string(),
        Some(b) => {
            let h = (chrono::Utc::now() - b.created_at).num_hours();
            format_age(h)
        }
    };

    let rows: Vec<BackupRow> = backups
        .iter()
        .map(|b| {
            let age_h = (chrono::Utc::now() - b.created_at).num_hours();
            BackupRow {
                filename: b.filename.clone(),
                created_at: b.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                size_human: backup_service::format_size(b.size_bytes),
                age_hours: age_h,
            }
        })
        .collect();

    let tmpl = BackupsTemplate {
        is_auth: true,
        username,
        admin_path: state.admin_path.clone(),
        active_page: "backups".to_string(),
        rows,
        status_class: pill.css_class().to_string(),
        status_label: pill.label().to_string(),
        last_backup_ago: last_ago,
        backup_count: backups.len(),
        flash,
    };

    Html(tmpl.render().unwrap_or_else(|e| {
        error!("Backups template render error: {}", e);
        "<p>Template error</p>".to_string()
    }))
}

// ────────────────────────────────────────────────────────────────────────────
// Обработчики маршрутов
// ────────────────────────────────────────────────────────────────────────────

/// GET {admin_path}/backups — страница управления резервными копиями.
pub async fn get_backups_page(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    render_page(&state, &jar, None).await
}

/// POST {admin_path}/backups/create — немедленно создаёт резервную копию.
/// После завершения (или ошибки) возвращает страницу с flash-сообщением.
pub async fn create_backup_now(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    let flash = match backup_service::create_backup().await {
        Ok(info) => {
            // Обновляем task_health — чтобы панель знала что задача отработала
            state.task_health.record_success("daily_db_backup").await;
            format!(
                "Backup created: {} ({}, {}ms)",
                info.filename,
                backup_service::format_size(info.size_bytes),
                info.duration_ms
            )
        }
        Err(e) => {
            error!("Manual backup failed: {}", e);
            state
                .task_health
                .record_error("daily_db_backup", &e.to_string())
                .await;
            format!("Backup failed: {}", e)
        }
    };

    render_page(&state, &jar, Some(flash)).await
}

/// GET {admin_path}/backups/{filename} — скачать файл резервной копии.
/// Читает файл в память — для сжатых дампов типичного размера (<50 MB) это нормально.
pub async fn download_backup(
    State(_state): State<AppState>,
    Path(filename): Path<String>,
) -> Response {
    match backup_service::backup_file_path(&filename) {
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
        Ok(path) => match tokio::fs::read(&path).await {
            Err(e) => {
                error!("Cannot read backup file {:?}: {}", path, e);
                (StatusCode::INTERNAL_SERVER_ERROR, "File read error").into_response()
            }
            Ok(bytes) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/gzip")
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename),
                )
                .body(Body::from(bytes))
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Response build error").into_response()
                }),
        },
    }
}

/// POST {admin_path}/backups/{filename}/delete — удалить резервную копию.
/// После удаления делает HTMX-redirect на страницу backups.
pub async fn delete_backup_handler(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Response {
    match backup_service::delete_backup(&filename).await {
        Ok(_) => {
            // HX-Redirect для HTMX; обычный браузер тоже следует 302
            Response::builder()
                .status(StatusCode::SEE_OTHER)
                .header("Location", format!("{}/backups", state.admin_path))
                .header("HX-Redirect", format!("{}/backups", state.admin_path))
                .body(Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            error!("Delete backup failed: {}", e);
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

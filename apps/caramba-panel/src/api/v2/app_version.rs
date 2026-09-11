//! `GET /api/v2/app/version` — последняя версия клиента для платформы — и
//! учёт версии установленного приложения по заголовку `X-Caramba-App-Version`.
//!
//! Публичный, как `/api/client/app/downloads`: приложение спрашивает его до
//! всякой авторизации (и в generic-режиме, где аккаунта нет вовсе), а
//! ответ не содержит ничего, кроме того, что и так лежит в открытом релизе
//! на GitHub. Через зеркало (`/api/*` проксируется) — адрес панели наружу не
//! уходит: `download_url` строится от публичной базы
//! (`app_enroll::public_origin`), то есть от зеркала, когда оно задано.
//!
//! Источник данных — `services::client_release_service`: манифест CI в
//! `downloads/`, иначе ручные настройки админки.

use axum::{
    Json,
    extract::{Query, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::services::client_release_service as releases;
use crate::services::subscription_service::DeviceIdentity;

use super::app_auth::AuthUser;

#[derive(Debug, Deserialize)]
pub struct VersionQuery {
    pub platform: Option<String>,
}

/// Ответ приложению. Поля, которых у ручного источника нет, отсутствуют
/// (`skip_serializing_if`), а не приходят пустыми строками: клиент и так
/// обязан уметь без них.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppVersionResponse {
    pub platform: String,
    pub version: String,
    pub build: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Минимальная сборка: ниже неё приложение показывает «Нужно обновиться».
    /// Ноль — не требовать.
    pub min_build: i64,
    /// «Что нового» из настройки панели (может быть пустым).
    pub notes: String,
    /// `manifest` | `manual` — для диагностики, приложение поле не читает.
    pub source: &'static str,
}

/// Сборка ответа — чистая функция, чтобы её можно было проверить без БД.
///
/// Ссылка на файл строится только когда есть и файл, и публичная база, и
/// база https: та же проверка, что у `/downloads` в мини-аппе. Настроенный
/// оператором `app_download_url_<platform>` сильнее собранной ссылки — он мог
/// намеренно увести загрузку в магазин или на другой хост.
pub fn build_response(
    latest: &releases::LatestClient,
    public_origin: Option<&str>,
    configured_download_url: &str,
    min_build: i64,
    notes: &str,
) -> AppVersionResponse {
    let configured = configured_download_url.trim();
    let download_url = if !configured.is_empty() && configured.starts_with("https://") {
        Some(configured.to_string())
    } else {
        match (latest.file.as_deref(), public_origin) {
            (Some(file), Some(origin)) if origin.starts_with("https://") => Some(format!(
                "{}/downloads/{}",
                origin.trim_end_matches('/'),
                file
            )),
            _ => None,
        }
    };
    AppVersionResponse {
        platform: latest.platform.clone(),
        version: latest.version.clone(),
        build: latest.build,
        download_url,
        size: latest.size,
        sha256: latest.sha256.clone().filter(|s| !s.is_empty()),
        published_at: latest.published_at.clone(),
        min_build,
        notes: notes.trim().to_string(),
        source: match latest.source {
            releases::LatestSource::Manifest => "manifest",
            releases::LatestSource::Manual => "manual",
        },
    }
}

/// GET /api/v2/app/version?platform=android|windows|macos|linux
///
/// 400 — платформа не названа или неизвестна; 404 — версии для неё нет ни в
/// манифесте, ни в настройках (приложение трактует это как «обновлений нет»).
pub async fn get_version(State(state): State<AppState>, Query(q): Query<VersionQuery>) -> Response {
    let platform = q
        .platform
        .as_deref()
        .map(|p| p.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !releases::is_known_platform(&platform) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "unknown platform" })),
        )
            .into_response();
    }

    let manifest = releases::cached_manifest(&platform);
    let manual_version = state
        .settings
        .get_or_default(releases::SETTING_LATEST_VERSION, "")
        .await;
    let manual_build = state
        .settings
        .get_or_default(releases::SETTING_LATEST_BUILD, "")
        .await;
    let Some(latest) =
        releases::resolve_latest(&platform, manifest.as_ref(), &manual_version, &manual_build)
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "no release for platform" })),
        )
            .into_response();
    };

    let public_origin = super::app_enroll::public_origin(&state).await;
    let configured = state
        .settings
        .get_or_default(&format!("app_download_url_{platform}"), "")
        .await;
    let min_build = releases::min_build_for(
        &state
            .settings
            .get_or_default(&format!("{}_{platform}", releases::SETTING_MIN_BUILD), "")
            .await,
        &state
            .settings
            .get_or_default(releases::SETTING_MIN_BUILD, "")
            .await,
    );
    let notes = state
        .settings
        .get_or_default(releases::SETTING_RELEASE_NOTES, "")
        .await;

    Json(build_response(
        &latest,
        public_origin.as_deref(),
        &configured,
        min_build,
        &notes,
    ))
    .into_response()
}

/// Middleware защищённых маршрутов: запоминает версию приложения в лизе
/// устройства аккаунта.
///
/// Стоит ВНУТРИ `require_app_jwt` (см. порядок `route_layer` в `mod.rs`):
/// без `AuthUser` в расширениях версия просто не пишется. Запись уходит в
/// фоновую задачу — ответ приложению её не ждёт, а ошибка БД остаётся в логе.
/// Значение заголовка недоверенное: только колонка для показа, никаких
/// решений по нему.
pub async fn record_app_version(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let version = releases::sanitize_app_version(
        req.headers()
            .get(DeviceIdentity::HEADER_APP_VERSION)
            .and_then(|v| v.to_str().ok()),
    );
    if let Some(version) = version
        && let Some(auth) = req.extensions().get::<AuthUser>()
    {
        let device = DeviceIdentity::from_headers(req.headers());
        if let Some(client_device_id) = device.client_device_id {
            let pool = state.pool.clone();
            let user_id = auth.user_id;
            tokio::spawn(async move {
                releases::record_device_app_version(&pool, user_id, &client_device_id, &version)
                    .await;
            });
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::client_release_service::{LatestClient, LatestSource};

    fn latest(file: Option<&str>) -> LatestClient {
        LatestClient {
            platform: "windows".into(),
            version: "1.0.0".into(),
            build: 110,
            file: file.map(str::to_string),
            size: file.map(|_| 42),
            sha256: file.map(|_| "ab".repeat(32)),
            published_at: Some("2026-09-11T00:00:00Z".into()),
            source: if file.is_some() {
                LatestSource::Manifest
            } else {
                LatestSource::Manual
            },
        }
    }

    /// Ссылка строится от публичной базы (зеркала), а не от панели, и только
    /// по https.
    #[test]
    fn download_url_is_built_from_the_public_origin() {
        let r = build_response(
            &latest(Some("Caramba-Connect-Setup-x64.exe")),
            Some("https://app.example.com/"),
            "",
            0,
            "  notes ",
        );
        assert_eq!(
            r.download_url.as_deref(),
            Some("https://app.example.com/downloads/Caramba-Connect-Setup-x64.exe")
        );
        assert_eq!(r.notes, "notes");
        assert_eq!(r.source, "manifest");
        assert_eq!(r.min_build, 0);

        let http = build_response(
            &latest(Some("x.exe")),
            Some("http://panel.example.com"),
            "",
            0,
            "",
        );
        assert!(http.download_url.is_none(), "http-база ссылку не даёт");
    }

    /// Настроенный адрес сильнее собранного; ручной источник без файла и без
    /// настройки — без ссылки, но с версией.
    #[test]
    fn configured_url_wins_and_manual_source_has_no_file() {
        let r = build_response(
            &latest(Some("x.exe")),
            Some("https://app.example.com"),
            " https://store.example.com/app ",
            105,
            "",
        );
        assert_eq!(
            r.download_url.as_deref(),
            Some("https://store.example.com/app")
        );
        assert_eq!(r.min_build, 105);

        let manual = build_response(&latest(None), Some("https://app.example.com"), "", 0, "");
        assert!(manual.download_url.is_none());
        assert_eq!(manual.source, "manual");
        assert_eq!(manual.build, 110);
        let json = serde_json::to_value(&manual).unwrap();
        assert!(
            json.get("download_url").is_none(),
            "пустые поля не сериализуются"
        );
        assert!(json.get("size").is_none());
    }
}

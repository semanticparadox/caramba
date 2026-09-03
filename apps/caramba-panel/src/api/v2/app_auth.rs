//! Аутентификация standalone-приложения (Flutter + Go-ядро mihomo).
//!
//! Поддерживает два способа входа:
//!   * email + password (bcrypt-хеш в `users.password_hash`);
//!   * Telegram Login Widget / WebApp (HMAC-SHA256 по bot_token).
//!
//! Панель выпускает пару JWT:
//!   * access-токен (~15 мин, HS256, секрет из env `APP_JWT_SECRET`);
//!   * refresh-токен (~30 дней) — хранится в БД ХЕШИРОВАННЫМ (sha256-hex) в
//!     таблице `refresh_tokens`, что позволяет ротировать и отзывать сессии.
//!
//! Секрет берётся ИМЕННО из `APP_JWT_SECRET` (а не из `session_secret`), чтобы
//! токены приложения были изолированы от admin/mini-app сессий панели.

use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Json},
};
use bcrypt::{DEFAULT_COST, hash, verify};
use caramba_db::repositories::user_repo::UserRepository;
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

// Время жизни токенов.
const ACCESS_TTL_SECS: i64 = 15 * 60; // 15 минут
const REFRESH_TTL_SECS: i64 = 30 * 24 * 60 * 60; // 30 дней

// Per-IP rate limit для публичных логин-эндпоинтов (login/code, login/email).
// Эти маршруты не закрыты JWT, а login/code перебирается по пространству 10^6
// при TTL 300с — без троттлинга атакующий может брутфорсить живые коды и
// заниматься credential stuffing по email. Окно узкое (fixed-window через
// Redis), общее для обоих эндпоинтов на один IP.
const LOGIN_RL_LIMIT: usize = 10;
const LOGIN_RL_WINDOW_SECS: usize = 60;

#[inline]
fn ensure_jwt_crypto_provider() {
    // Идемпотентная установка crypto-провайдера для jsonwebtoken 10.x.
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();
}

/// JWT-claims access-токена приложения.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppClaims {
    /// ID пользователя (users.id) в виде строки.
    pub sub: String,
    /// Время истечения (unix-секунды).
    pub exp: usize,
    /// Время выпуска (unix-секунды).
    pub iat: usize,
    /// Тип токена: всегда "access" для этого claims-набора.
    pub typ: String,
}

/// Извлечённый аутентифицированный пользователь — кладётся в extensions
/// middleware'ом `require_app_jwt` и достаётся хендлерами через `Extension`.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
}

/// Секрет для подписи JWT приложения. Берётся из `APP_JWT_SECRET`;
/// при отсутствии падаем на `SESSION_SECRET`, чтобы не уронить сервис в dev.
fn jwt_secret() -> String {
    std::env::var("APP_JWT_SECRET")
        .ok()
        .filter(|s| s.len() >= 32)
        .or_else(|| std::env::var("SESSION_SECRET").ok())
        .unwrap_or_default()
}

/// Хеширует refresh-токен для хранения в БД (sha256-hex).
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Генерирует случайный непрозрачный refresh-токен (256 бит, hex).
fn generate_refresh_token() -> String {
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Выпускает access-JWT для пользователя.
fn issue_access_token(user_id: i64) -> Result<String, StatusCode> {
    ensure_jwt_crypto_provider();
    let now = chrono::Utc::now().timestamp();
    let claims = AppClaims {
        sub: user_id.to_string(),
        iat: now as usize,
        exp: (now + ACCESS_TTL_SECS) as usize,
        typ: "access".to_string(),
    };
    let secret = jwt_secret();
    if secret.is_empty() {
        tracing::error!("APP_JWT_SECRET/SESSION_SECRET not configured");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
    })) {
        Ok(Ok(token)) => Ok(token),
        Ok(Err(e)) => {
            tracing::error!(err = %e, "app jwt encode failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(_) => {
            tracing::error!("jsonwebtoken panicked while encoding app token");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Выпускает и сохраняет refresh-токен (хеш) для сессии пользователя.
async fn issue_refresh_token(
    state: &AppState,
    user_id: i64,
    user_agent: Option<&str>,
) -> Result<String, StatusCode> {
    let token = generate_refresh_token();
    let token_hash = hash_token(&token);
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(REFRESH_TTL_SECS);

    sqlx::query(
        "INSERT INTO refresh_tokens (user_id, token_hash, expires_at, user_agent) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(&token_hash)
    .bind(expires_at)
    .bind(user_agent)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!(err = %e, "failed to persist refresh token");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(token)
}

/// Выпускает пару (access + refresh) для пользователя — единый путь выпуска
/// токенов, общий для login_email / login_telegram / login_code. Гарантирует,
/// что все способы входа отдают идентичный success-JSON (см. `token_pair`).
async fn issue_session(
    state: &AppState,
    user_id: i64,
    headers: &HeaderMap,
) -> Result<TokenPair, StatusCode> {
    let access = issue_access_token(user_id)?;
    let refresh = issue_refresh_token(state, user_id, user_agent_of(headers).as_deref()).await?;
    Ok(token_pair(access, refresh, user_id))
}

// ============================================================
// DTO
// ============================================================

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub full_name: Option<String>,
    /// Опциональный код вовлечения (enrollment) из диплинка carambaconnect://enroll.
    /// Списывается при создании аккаунта (одна транзакция, atomic). Если задан, но
    /// невалиден — регистрация отклоняется с 400 (явный UX). Отсутствие кода =
    /// обычная регистрация без побочных эффектов (сохраняем поведение для live-юзеров).
    pub enroll_code: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginEmailRequest {
    pub email: String,
    pub password: String,
}

/// Логин через Telegram Login Widget / WebApp.
/// Передаётся либо сырой `init_data` (WebApp), либо плоские поля Login Widget.
#[derive(Deserialize)]
pub struct LoginTelegramRequest {
    /// Сырой initData из Telegram WebApp (приоритетный путь).
    #[serde(alias = "initData")]
    pub init_data: Option<String>,
    /// Поля Telegram Login Widget (когда initData недоступен).
    pub id: Option<i64>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub username: Option<String>,
    /// URL аватара — Telegram присылает его, если у пользователя есть фото.
    pub photo_url: Option<String>,
    pub auth_date: Option<i64>,
    pub hash: Option<String>,
    /// Опциональный код вовлечения (enrollment). Списывается ТОЛЬКО когда этот
    /// логин создаёт нового пользователя (первый upsert по tg_id). Для уже
    /// существующего аккаунта игнорируется (не fresh-account path).
    pub enroll_code: Option<String>,
    /// Любые прочие поля Login Widget — нужны для построения data-check-string
    /// (Telegram требует включать в DCS ВСЕ полученные поля, кроме `hash`).
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Логин по одноразовому коду из Telegram-бота.
/// Код генерируется ботом по /login и хранится в Redis ("app:logincode:{code}").
#[derive(Deserialize)]
pub struct LoginCodeRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub user_id: i64,
}

fn token_pair(access: String, refresh: String, user_id: i64) -> TokenPair {
    TokenPair {
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer",
        expires_in: ACCESS_TTL_SECS,
        user_id,
    }
}

/// Извлекает IP клиента из заголовков обратного прокси (Cloudflare / Caddy /
/// Nginx). Тот же порядок приоритета, что и в admin/auth.rs::extract_login_ip.
fn extract_client_ip(headers: &HeaderMap) -> String {
    headers
        .get("cf-connecting-ip")
        .or_else(|| headers.get("x-real-ip"))
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Проверяет per-IP rate limit для публичных логин-эндпоинтов.
///
/// Возвращает `Ok(())` если запрос разрешён, либо `Err(429)` если лимит
/// превышен. При недоступности Redis пропускаем запрос (доступность важнее
/// защиты при деградации инфраструктуры) — то же поведение, что у
/// bot_rate_limit и admin/auth. Ключ общий для всех логин-эндпоинтов одного IP.
async fn check_login_rate_limit(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let ip = extract_client_ip(headers);
    let key = format!("app:loginrl:{}", ip);
    match state
        .redis
        .check_rate_limit(&key, LOGIN_RL_LIMIT, LOGIN_RL_WINDOW_SECS)
        .await
    {
        Ok(true) => Ok(()),
        Ok(false) => {
            tracing::warn!(ip = %ip, "app login rate limit exceeded");
            Err(StatusCode::TOO_MANY_REQUESTS)
        }
        Err(e) => {
            tracing::error!(err = %e, ip = %ip, "app login rate-limit check failed — allowing request");
            Ok(())
        }
    }
}

fn user_agent_of(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(255).collect())
}

/// Простейшая валидация email — базовый формат `a@b.c`.
fn is_valid_email(email: &str) -> bool {
    let email = email.trim();
    if email.len() < 3 || email.len() > 254 {
        return false;
    }
    match email.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        None => false,
    }
}

/// Проверяет, что ошибка (обёрнутая anyhow) — это Postgres unique-violation (23505).
/// Нужна, чтобы отличить «email уже занят» на гонке INSERT от настоящего сбоя БД.
fn is_unique_violation(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
        .filter_map(|e| e.as_database_error())
        .any(|db| db.code().as_deref() == Some("23505"))
}

// ============================================================
// MIDDLEWARE
// ============================================================

/// Middleware: валидирует Bearer access-JWT и кладёт `AuthUser` в extensions.
/// Подключается через `axum::middleware::from_fn`.
pub async fn require_app_jwt(
    mut req: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    ensure_jwt_crypto_provider();

    let token = match req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        Some(h) if h.starts_with("Bearer ") => h[7..].to_string(),
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    let secret = jwt_secret();
    if secret.is_empty() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    let data = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode::<AppClaims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
    })) {
        Ok(Ok(d)) => d,
        Ok(Err(_)) => return Err(StatusCode::UNAUTHORIZED),
        Err(_) => {
            tracing::error!("jsonwebtoken panicked while decoding app token");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Принимаем только access-токены — refresh нельзя использовать как Bearer.
    if data.claims.typ != "access" {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let user_id: i64 = match data.claims.sub.parse() {
        Ok(v) => v,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    req.extensions_mut().insert(AuthUser { user_id });
    Ok(next.run(req).await)
}

// ============================================================
// HANDLERS — публичные (без JWT)
// ============================================================

/// POST /api/v2/app/register — регистрация по email/password.
pub async fn register_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    // Канонизируем email в нижний регистр — уникальность гарантирует partial
    // unique index idx_users_email_lower ON users(LOWER(email)), поэтому в БД
    // должно лежать каноническое значение, а не то, что юзер набрал руками.
    let email = payload.email.trim().to_lowercase();
    if !is_valid_email(&email) {
        return (StatusCode::BAD_REQUEST, "Invalid email").into_response();
    }
    if payload.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            "Password must be at least 8 characters",
        )
            .into_response();
    }

    let repo = UserRepository::new(state.pool.clone());

    // Проверяем, что email ещё не занят.
    match repo.find_by_email(&email).await {
        Ok(Some(_)) => return (StatusCode::CONFLICT, "Email already registered").into_response(),
        Ok(None) => {}
        Err(e) => {
            tracing::error!(err = %e, "register: email lookup failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    // Нормализуем enroll_code один раз: trimmed непустая строка => Some, иначе None.
    // None означает обычную регистрацию без кода (поведение для ~20 живых юзеров
    // не меняется).
    let enroll_code = payload
        .enroll_code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());

    // Пред-валидация кода ДО создания аккаунта (major-1). Если код передан, но
    // невалиден/истёк/исчерпан — отвечаем 400 и НЕ создаём аккаунт. Раньше юзер
    // уже коммитился, повтор с верным кодом упирался в 409, и валидный код было
    // невозможно списать. Это read-only проверка (use не потребляется); фактическое
    // списание идёт в одной транзакции с INSERT через register_email_with_enroll.
    if let Some(code) = enroll_code {
        match state.store_service.validate_enrollment_code(code).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    "Invalid or expired enrollment code",
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!(err = %e, "register: enroll pre-validation failed");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }

    let password_hash = match hash(&payload.password, DEFAULT_COST) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(err = %e, "register: bcrypt hashing failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Уникальный referral_code на основе случайного суффикса (users.referral_code UNIQUE).
    let referral_code = format!("app{}", &generate_refresh_token()[..12]);

    let user = if let Some(code) = enroll_code {
        // Атомарный путь: создание пользователя И списание кода в ОДНОЙ транзакции
        // (major-1). Невалидный код или сбой redeem откатывают INSERT — аккаунт не
        // создаётся, клиент может повторить (email ещё свободен). Между пред-
        // валидацией и этим вызовом код мог исчерпаться гонкой — тогда Ok(None) и
        // мы так же отдаём 400 без созданного аккаунта.
        match state
            .store_service
            .register_email_with_enroll(
                &email,
                &password_hash,
                payload.full_name.as_deref(),
                &referral_code,
                code,
            )
            .await
        {
            Ok(Some(u)) => u,
            Ok(None) => {
                tracing::warn!("register: enroll_code invalid/exhausted at redemption");
                return (
                    StatusCode::BAD_REQUEST,
                    "Invalid or expired enrollment code",
                )
                    .into_response();
            }
            Err(e) => {
                // Гонка по email (unique index idx_users_email_lower, код 23505) =>
                // 409, как и в безкодовом пути. Прочее => 500. Аккаунт в обоих
                // случаях не создан (tx откатился).
                if is_unique_violation(&e) {
                    return (StatusCode::CONFLICT, "Email already registered").into_response();
                }
                tracing::error!(err = %e, "register: atomic enroll registration failed");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    } else {
        // Безкодовый путь без изменений: обычная регистрация через пул.
        match repo
            .create_email_user(
                &email,
                &password_hash,
                payload.full_name.as_deref(),
                &referral_code,
            )
            .await
        {
            Ok(u) => u,
            Err(e) => {
                // Гонка: два одновременных register одного email (отличающегося
                // регистром) проходят find_by_email, но второй INSERT упирается в
                // unique index idx_users_email_lower (Postgres код 23505). Отдаём
                // 409, а не 500 — это нормальный «email занят», а не сбой.
                if is_unique_violation(&e) {
                    return (StatusCode::CONFLICT, "Email already registered").into_response();
                }
                tracing::error!(err = %e, "register: create_email_user failed");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    // Бесплатный план новому аккаунту.
    //
    // Раньше его выдавал ТОЛЬКО путь с инвайт-кодом (register_email_with_enroll →
    // ensure_free_plan_subscription_tx) и бот. Обычная регистрация из приложения
    // не выдавала ничего: человек заводил аккаунт и оставался без подписки, то
    // есть без строки в конфиге любой ноды, и не мог ни подключиться, ни дойти до
    // экрана оплаты. Теперь оба пути ведут к одному состоянию.
    //
    // Идемпотентно: на кодовом пути подписка уже создана в транзакции, и вызов
    // вернёт None. Раскатку по нодам делаем в обоих случаях, потому что нового
    // участника бесплатного плана ноды не увидят, пока конфиг не перевыпущен.
    grant_free_plan_on_signup(&state, user.id).await;

    let access = match issue_access_token(user.id) {
        Ok(t) => t,
        Err(s) => return s.into_response(),
    };
    let refresh =
        match issue_refresh_token(&state, user.id, user_agent_of(&headers).as_deref()).await {
            Ok(t) => t,
            Err(s) => return s.into_response(),
        };

    (
        StatusCode::CREATED,
        Json(token_pair(access, refresh, user.id)),
    )
        .into_response()
}

/// Сажает новый аккаунт на бесплатный план и публикует конфиг плана на ноды.
///
/// Best-effort по построению: аккаунт уже создан и токены будут выданы в любом
/// случае. Отсутствие настроенного бесплатного плана это конфигурация оператора,
/// а не сбой регистрации, поэтому здесь предупреждение, а не ошибка ответа.
async fn grant_free_plan_on_signup(state: &AppState, user_id: i64) {
    let granted = match state.store_service.ensure_free_plan_subscription(user_id).await {
        Ok(plan) => plan,
        Err(e) => {
            tracing::warn!(user_id, error = %e, "signup: free plan grant failed (non-fatal)");
            None
        }
    };

    // На кодовом пути подписка создана в транзакции регистрации и вызов выше
    // вернул None; публиковать всё равно надо, поэтому берём план напрямую.
    let plan_id = match granted {
        Some(id) => Some(id),
        None => sqlx::query_scalar::<_, i64>(
            "SELECT p.id FROM plans p \
             JOIN subscriptions s ON s.plan_id = p.id \
             WHERE s.user_id = $1 AND p.is_free AND p.is_active \
               AND s.status IN ('active', 'pending', 'throttled') \
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None),
    };

    let Some(plan_id) = plan_id else {
        tracing::warn!(
            user_id,
            "signup: no active free plan configured, account created without a subscription"
        );
        return;
    };

    if let Err(e) = state
        .orchestration_service
        .notify_nodes_for_plans(&[plan_id])
        .await
    {
        tracing::warn!(
            user_id,
            plan_id,
            error = %e,
            "signup: free plan granted but node publish failed (non-fatal)"
        );
    }
}

/// POST /api/v2/app/login/email — вход по email/password.
pub async fn login_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginEmailRequest>,
) -> impl IntoResponse {
    // Per-IP throttle против credential stuffing (эндпоинт публичный, без JWT).
    if let Err(s) = check_login_rate_limit(&state, &headers).await {
        return s.into_response();
    }

    let email = payload.email.trim();
    let repo = UserRepository::new(state.pool.clone());

    let (user_id, stored_hash) = match repo.get_credentials_by_email(email).await {
        Ok(Some((id, Some(h)))) => (id, h),
        // Аккаунт без пароля (например, чисто Telegram) либо отсутствует —
        // отвечаем единообразно, чтобы не раскрывать существование email.
        Ok(_) => return (StatusCode::UNAUTHORIZED, "Invalid credentials").into_response(),
        Err(e) => {
            tracing::error!(err = %e, "login: credentials lookup failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let ok = verify(&payload.password, &stored_hash).unwrap_or(false);
    if !ok {
        return (StatusCode::UNAUTHORIZED, "Invalid credentials").into_response();
    }

    match issue_session(&state, user_id, &headers).await {
        Ok(pair) => Json(pair).into_response(),
        Err(s) => s.into_response(),
    }
}

/// POST /api/v2/app/login/telegram — вход через Telegram (Login Widget / WebApp).
///
/// Проверяем HMAC-SHA256 подпись по bot_token (тот же алгоритм, что и в
/// api/client.rs::auth_telegram). Поддерживаем оба формата: сырой initData
/// (WebApp) и плоские поля Login Widget.
pub async fn login_telegram(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginTelegramRequest>,
) -> impl IntoResponse {
    let bot_token = state.settings.get_or_default("bot_token", "").await;
    if bot_token.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Bot token not configured",
        )
            .into_response();
    }

    // Разбираем входные данные в (params, hash, tg_id, secret_key_mode).
    // WebApp: secret_key = HMAC("WebAppData", bot_token).
    // Login Widget: secret_key = SHA256(bot_token).
    let (data_check_string, provided_hash, tg_id, is_webapp) =
        if let Some(init_data) = payload.init_data.as_ref().filter(|s| !s.is_empty()) {
            // --- Telegram WebApp initData ---
            let mut params: HashMap<String, String> = HashMap::new();
            for (k, v) in url::form_urlencoded::parse(init_data.as_bytes()) {
                params.insert(k.into_owned(), v.into_owned());
            }
            let hash = match params.get("hash") {
                Some(h) => h.clone(),
                None => return (StatusCode::BAD_REQUEST, "Missing hash").into_response(),
            };
            // Проверка свежести (24ч) — защита от replay.
            if let Some(ad) = params.get("auth_date").and_then(|s| s.parse::<i64>().ok()) {
                let age = chrono::Utc::now().timestamp() - ad;
                if !(0..=86_400).contains(&age) {
                    return (StatusCode::UNAUTHORIZED, "InitData expired").into_response();
                }
            } else {
                return (StatusCode::BAD_REQUEST, "Missing auth_date").into_response();
            }
            let user_json = match params
                .get("user")
                .and_then(|u| serde_json::from_str::<serde_json::Value>(u).ok())
            {
                Some(v) => v,
                None => return (StatusCode::BAD_REQUEST, "Missing user data").into_response(),
            };
            let tg_id = match user_json.get("id").and_then(|v| v.as_i64()) {
                Some(id) => id,
                None => return (StatusCode::BAD_REQUEST, "Missing user ID").into_response(),
            };
            let mut pairs: Vec<String> = params
                .iter()
                .filter(|(k, _)| k.as_str() != "hash")
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            pairs.sort();
            (pairs.join("\n"), hash, tg_id, true)
        } else {
            // --- Telegram Login Widget (плоские поля) ---
            let hash = match payload.hash.clone() {
                Some(h) if !h.is_empty() => h,
                _ => return (StatusCode::BAD_REQUEST, "Missing hash").into_response(),
            };
            let tg_id = match payload.id {
                Some(id) => id,
                None => return (StatusCode::BAD_REQUEST, "Missing user ID").into_response(),
            };
            if let Some(ad) = payload.auth_date {
                let age = chrono::Utc::now().timestamp() - ad;
                if !(0..=86_400).contains(&age) {
                    return (StatusCode::UNAUTHORIZED, "Auth data expired").into_response();
                }
            } else {
                return (StatusCode::BAD_REQUEST, "Missing auth_date").into_response();
            }
            // data-check-string из ВСЕХ полученных полей, кроме `hash`,
            // отсортированных по ключу (требование спецификации Telegram).
            // Иначе для пользователей с аватаром (photo_url) HMAC не сойдётся.
            let mut fields: Vec<(String, String)> = Vec::new();
            if let Some(ad) = payload.auth_date {
                fields.push(("auth_date".into(), ad.to_string()));
            }
            if let Some(v) = payload.first_name.as_ref() {
                fields.push(("first_name".into(), v.clone()));
            }
            fields.push(("id".into(), tg_id.to_string()));
            if let Some(v) = payload.last_name.as_ref() {
                fields.push(("last_name".into(), v.clone()));
            }
            if let Some(v) = payload.username.as_ref() {
                fields.push(("username".into(), v.clone()));
            }
            if let Some(v) = payload.photo_url.as_ref() {
                fields.push(("photo_url".into(), v.clone()));
            }
            // Прочие поля из catch-all (на случай будущих добавлений Telegram).
            for (k, v) in payload.extra.iter() {
                if k == "hash" {
                    continue;
                }
                // Строкам не добавляем кавычки; остальные значения сериализуем как JSON.
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                fields.push((k.clone(), s));
            }
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            let dcs = fields
                .into_iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("\n");
            (dcs, hash, tg_id, false)
        };

    // Вычисляем ожидаемый HMAC.
    let secret_key: Vec<u8> = if is_webapp {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"WebAppData").unwrap();
        mac.update(bot_token.as_bytes());
        mac.finalize().into_bytes().to_vec()
    } else {
        let mut hasher = Sha256::new();
        hasher.update(bot_token.as_bytes());
        hasher.finalize().to_vec()
    };
    let calculated = {
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret_key).unwrap();
        mac.update(data_check_string.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    };
    if calculated != provided_hash {
        tracing::warn!("app telegram login: hash mismatch");
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }

    // Находим/создаём пользователя по tg_id (upsert как в боте).
    let repo = UserRepository::new(state.pool.clone());
    let full_name = match (&payload.first_name, &payload.last_name) {
        (Some(f), Some(l)) => Some(format!("{} {}", f, l).trim().to_string()),
        (Some(f), None) => Some(f.clone()),
        _ => None,
    };
    // Признак fresh-account: существовал ли пользователь до upsert. Нужен, чтобы
    // списывать enroll_code ТОЛЬКО при реальном создании аккаунта (повторный
    // логин по существующему tg_id код не потребляет).
    let was_new = match repo.get_by_tg_id(tg_id).await {
        Ok(existing) => existing.is_none(),
        Err(e) => {
            tracing::error!(err = %e, "app telegram login: existence check failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let user = match repo
        .upsert(
            tg_id,
            payload.username.as_deref(),
            full_name.as_deref(),
            None,
        )
        .await
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "app telegram login: upsert failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Новый аккаунт через Telegram это тоже регистрация: без подписки он не
    // попадёт ни в один конфиг ноды. Порядок с enroll_code не важен, вызов
    // идемпотентен.
    if was_new {
        grant_free_plan_on_signup(&state, user.id).await;
    }

    // Списываем enroll_code только при создании нового аккаунта. Для Telegram-входа
    // невалидный код НЕ блокирует логин (в отличие от register): диплинк-вход —
    // не строго регистрационный путь, отказ ухудшил бы UX уже валидного юзера.
    // Логируем и продолжаем.
    if was_new && let Some(raw_code) = payload.enroll_code.as_deref() {
        let code = raw_code.trim();
        if !code.is_empty() {
            match state
                .store_service
                .redeem_enrollment_code(user.id, code)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        user_id = user.id,
                        "telegram login: enroll_code invalid/exhausted (ignored)"
                    );
                }
                Err(e) => {
                    tracing::error!(err = %e, user_id = user.id, "telegram login: enroll redemption failed (ignored)");
                }
            }
        }
    }

    match issue_session(&state, user.id, &headers).await {
        Ok(pair) => Json(pair).into_response(),
        Err(s) => s.into_response(),
    }
}

/// POST /api/v2/app/login/code — вход по одноразовому коду из Telegram-бота.
///
/// Бот по /login кладёт в Redis ключ "app:logincode:{code}" => tg_id (TTL 300с,
/// одноразовый). Здесь мы атомарно ищем-и-удаляем код (GETDEL, single-use),
/// резолвим tg_id -> users.id и выпускаем ту же пару JWT, что и
/// login_email/login_telegram (общий путь `issue_session`). Ответ идентичен
/// прочим логинам, чтобы клиенты использовали один парсер. На промах/просрочку —
/// 401 без деталей.
///
/// Brute-force защита: per-IP rate limit (`check_login_rate_limit`) — то же
/// окно, что и у login_email. На промахе не раскрываем, что именно не сошлось.
pub async fn login_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginCodeRequest>,
) -> impl IntoResponse {
    // Per-IP throttle: пространство кода 10^6, без лимита его можно перебрать.
    if let Err(s) = check_login_rate_limit(&state, &headers).await {
        return s.into_response();
    }

    // Принимаем строго 6 цифр — отсекаем мусор до удара по Redis.
    let code = payload.code.trim();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return (StatusCode::UNAUTHORIZED, "Invalid or expired code").into_response();
    }

    let redis_key = format!("app:logincode:{}", code);

    // Атомарно читаем-и-удаляем код (GETDEL): гарантирует single-use даже при
    // конкурентных запросах с одним кодом — tg_id получит только первый. Ошибку
    // Redis НЕ превращаем в 401 (это не «код неверный», а сбой инфраструктуры) —
    // отдаём 500, чтобы клиент мог повторить.
    let tg_id_str = match state.redis.get_del(&redis_key).await {
        Ok(Some(v)) => v,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "Invalid or expired code").into_response(),
        Err(e) => {
            tracing::error!(err = %e, "login_code: redis getdel failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let tg_id: i64 = match tg_id_str.trim().parse() {
        Ok(v) => v,
        Err(_) => {
            tracing::error!(value = %tg_id_str, "login_code: malformed tg_id in redis");
            return (StatusCode::UNAUTHORIZED, "Invalid or expired code").into_response();
        }
    };

    // Резолвим tg_id -> users.id. Пользователь обязан существовать (бот делает
    // upsert на /start). Если строки нет — отвечаем единообразно (401), не
    // раскрывая, что именно не сошлось.
    let repo = UserRepository::new(state.pool.clone());
    let user = match repo.get_by_tg_id(tg_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            tracing::warn!(tg_id, "login_code: no user for tg_id");
            return (StatusCode::UNAUTHORIZED, "Invalid or expired code").into_response();
        }
        Err(e) => {
            tracing::error!(err = %e, "login_code: user lookup failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match issue_session(&state, user.id, &headers).await {
        Ok(pair) => Json(pair).into_response(),
        Err(s) => s.into_response(),
    }
}

/// POST /api/v2/app/refresh — ротация refresh-токена и выпуск нового access.
///
/// Старый refresh помечается revoked, выдаётся новая пара (rotation).
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RefreshRequest>,
) -> impl IntoResponse {
    let token_hash = hash_token(&payload.refresh_token);

    // Находим живой (не отозванный, не истёкший) токен.
    // Ошибку БД НЕ глотаем: транзиентный сбой Postgres не должен превращаться в
    // 401 «токен невалиден» (это выкинуло бы клиента из аккаунта по живому
    // токену). Только Ok(None) — это действительно невалидный/истёкший токен.
    let row: Option<(i64, i64)> = match sqlx::query_as(
        "SELECT id, user_id FROM refresh_tokens \
         WHERE token_hash = $1 AND revoked = FALSE AND expires_at > now()",
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(err = %e, "refresh: token lookup failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let (token_id, user_id) = match row {
        Some(v) => v,
        None => return (StatusCode::UNAUTHORIZED, "Invalid refresh token").into_response(),
    };

    // Отзываем старый токен (rotation).
    if let Err(e) = sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE id = $1")
        .bind(token_id)
        .execute(&state.pool)
        .await
    {
        tracing::error!(err = %e, "refresh: failed to revoke old token");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let access = match issue_access_token(user_id) {
        Ok(t) => t,
        Err(s) => return s.into_response(),
    };
    let new_refresh =
        match issue_refresh_token(&state, user_id, user_agent_of(&headers).as_deref()).await {
            Ok(t) => t,
            Err(s) => return s.into_response(),
        };

    Json(token_pair(access, new_refresh, user_id)).into_response()
}

/// POST /api/v2/app/logout — отзыв конкретного refresh-токена.
pub async fn logout(
    State(state): State<AppState>,
    Json(payload): Json<LogoutRequest>,
) -> impl IntoResponse {
    let token_hash = hash_token(&payload.refresh_token);
    let _ = sqlx::query("UPDATE refresh_tokens SET revoked = TRUE WHERE token_hash = $1")
        .bind(&token_hash)
        .execute(&state.pool)
        .await;
    // Всегда 200 — logout идемпотентен и не раскрывает существование токена.
    Json(serde_json::json!({ "ok": true })).into_response()
}

//! Маршруты CSM/1 (`03-WIRE.md` раздел 13).
//!
//! Отдача документа это ЧТЕНИЕ, а не подписание. Причина в `03-WIRE.md` 1.5:
//! `iat` и `exp` входят в подписываемый payload, поэтому пересборка в другую
//! секунду даёт другой кадр и другой хэш, а хэш каталога опубликован в
//! ключевом документе. Подписывать на каждый запрос значило бы обесценивать
//! эту привязку при каждом перезапуске панели.
//!
//! Два исключения из этого правила:
//!   * директива подписывается на каждый запрос, потому что несёт nonce
//!     запросившего устройства (12.3), и это единственная свежесть, которая
//!     переживает неверные часы клиента;
//!   * ключевой документ подписывается на лету только потому, что его
//!     содержимое меняется редко и целиком лежит в базе.
//!
//! Тела отказов пусты (13.5). Причина отказа, которую клиент вправе показать
//! пользователю, едет подписанным полем `st`/`rc` внутри директивы с кодом
//! 200, а не текстом, которому нечем доверять: отозванная подписка получает
//! подписанный документ, а не голый 403.

use axum::{
    Router,
    body::Body,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use caramba_shared::csm::directive::{DeviceThumbprint, Locator, Nonce};
use caramba_shared::csm::{self, DocType};
use ed25519_dalek::SigningKey;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::catalog_store::{self as store};
use super::{SigningKeys, TenantIdentity, hex};
use crate::AppState;

/// Тип содержимого подписанных документов протокола (`03-WIRE.md` 13.4).
pub const CSM_CONTENT_TYPE: &str = "application/vnd.caramba.csm1";

/// Заголовок допуска к частям каталога: путь остаётся ключом кэша CDN, а
/// право чтения едет отдельно (13.2).
pub const HEADER_LOC: &str = "x-csm-loc";

/// Кэширование частей: `private`, потому что допуск по заголовку, а общий
/// кэш ключуется только по URL, и `public` отдал бы флот любому, кто видел
/// `cat_id`.
const CHUNK_CACHE_CONTROL: &str = "private, max-age=86400, immutable";
const KEY_CACHE_CONTROL: &str = "public, max-age=300";
const NO_STORE: &str = "no-store";

/// Пределы 13.3: на локатор и на IP, окно час.
const RATE_WINDOW: usize = 3600;
const M1_PER_LOC: usize = 60;
const M1_PER_IP: usize = 600;
const C1_PER_LOC: usize = 128;
const C1_PER_IP: usize = 1280;

/// Маршруты протокола. Монтируются в корень рядом с `/sub/{uuid}`, потому что
/// периметр `caramba-sub` проксирует именно `/sub/*` и `/api/*`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sub/k1", get(key_document))
        .route("/sub/c1/{cat_id}/{i}", get(catalog_chunk))
        .route("/sub/m1/{loc}", get(directive))
}

/// Верхний сервис панели (`03-WIRE.md` 13.1): уже обёрнутый слоями роутер
/// панели плюс маршруты CSM, которые в эти слои не попадают.
///
/// Сжатие превращает набивку в чистый расход, делая размер на проводе
/// функцией открытого текста (12.4), а пять постоянных заголовков
/// безопасности это отпечаток «это панель Caramba» на каждом ответе (13.4).
/// Порядок слияния и есть защита: слои уже применены к `layered_panel`, и
/// `csm` они не касаются. Тест ниже гоняет собранный сервис с
/// `Accept-Encoding` и проверяет точный набор заголовков.
pub fn mount(layered_panel: Router, csm: Router) -> Router {
    Router::new().merge(layered_panel).merge(csm)
}

// ---------------------------------------------------------------- ответы

/// Ответ 200 с одним кадром и ровно теми заголовками, что перечислены в
/// 13.4. `Content-Length` и `Date` ставит сервер.
fn frame_response(frame: Vec<u8>, cache_control: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static(CSM_CONTENT_TYPE),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        )
        .body(Body::from(frame))
        .expect("статические заголовки валидны")
}

/// Ответ с частью каталога: адресуется по содержимому, поэтому валидатор
/// сильный, а `Vary` называет заголовок допуска.
pub fn chunk_response(frame: Vec<u8>, etag: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static(CSM_CONTENT_TYPE),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static(CHUNK_CACHE_CONTROL),
        )
        .header(
            header::ETAG,
            HeaderValue::from_str(etag).expect("etag ascii"),
        )
        .header(header::VARY, HeaderValue::from_static("X-CSM-Loc"))
        .body(Body::from(frame))
        .expect("статические заголовки валидны")
}

/// 304 на условный GET части: тело пустое, валидатор повторён.
pub fn not_modified(etag: &str) -> Response {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(
            header::ETAG,
            HeaderValue::from_str(etag).expect("etag ascii"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static(CHUNK_CACHE_CONTROL),
        )
        .header(header::VARY, HeaderValue::from_static("X-CSM-Loc"))
        .body(Body::empty())
        .expect("статические заголовки валидны")
}

/// Тело ошибки пустое намеренно (`03-WIRE.md` 13.5).
fn refuse(status: StatusCode) -> Response {
    status.into_response()
}

// ---------------------------------------------------------------- общее

/// IP клиента за обратным прокси, как его видит легаси-маршрут подписки.
fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("cf-connecting-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("0.0.0.0")
        .to_string()
}

/// Лимитер маршрута. Закрывается при ошибке Redis (13.3): открытый лимитер на
/// маршруте с локатором в ключе это окно перебора при каждом сбое Redis.
///
/// В лог уходят маршрут и класс ключа, но не сам ключ: субъект класса `loc`
/// это учётные данные, и сбой Redis это ровно тот момент, когда они бы
/// высыпались в журнал всем потоком.
async fn rate_limit(
    state: &AppState,
    route: &'static str,
    class: &'static str,
    subject: &str,
    limit: usize,
) -> Result<(), StatusCode> {
    let key = format!("rate:csm:{route}:{class}:{subject}");
    match state.redis.check_rate_limit(&key, limit, RATE_WINDOW).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(StatusCode::TOO_MANY_REQUESTS),
        Err(e) => {
            tracing::error!(error = %e, route, class, "csm: лимитер недоступен, отказ");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// Субъект лимитера по локатору: хэш, а не сам локатор, чтобы учётные данные
/// не лежали в Redis открытым текстом. Ключуется на присланное значение и для
/// неизвестных локаторов тоже: перебор тогда ограничен только ключом по IP,
/// и это приемлемо при 120 битах локатора, но ключ по IP из-за этого убирать
/// нельзя.
fn locator_subject(loc: &Locator) -> String {
    let mut h = Sha256::new();
    h.update(b"csm-rl");
    h.update(loc.as_str().as_bytes());
    hex(&h.finalize()[..16])
}

/// Личность и онлайн-ключ, или 503 с названной причиной. Личность читается
/// на каждый запрос: оператор включает протокол во время работы панели, и
/// два индексных чтения дешевле кэша, который надо инвалидировать.
async fn load_signer(state: &AppState, route: &'static str) -> Result<SigningKeys, StatusCode> {
    match SigningKeys::load(&state.pool).await {
        Ok(Some(keys)) => Ok(keys),
        Ok(None) => {
            tracing::warn!(
                route,
                "csm: у тенанта нет корневого ключа, протокол не включён"
            );
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
        Err(e) => {
            tracing::error!(route, error = %e, "csm: загрузка ключей");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// Личность тенанта или 503. Для чтения хранимых кадров онлайн-ключ не
/// нужен, и его отсутствие или незарегистрированность не должны валить
/// маршрут, который ничего не подписывает.
async fn load_identity(
    state: &AppState,
    route: &'static str,
) -> Result<TenantIdentity, StatusCode> {
    match super::keys::load_identity(&state.pool).await {
        Ok(Some(identity)) => Ok(identity),
        Ok(None) => {
            tracing::warn!(
                route,
                "csm: у тенанта нет корневого ключа, протокол не включён"
            );
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
        Err(e) => {
            tracing::error!(route, error = %e, "csm: загрузка личности");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

/// Онлайн-ключ из загруженной связки или 503 с тенантом в логе.
fn online_key<'a>(
    keys: &'a SigningKeys,
    route: &'static str,
) -> Result<&'a SigningKey, StatusCode> {
    keys.online.as_ref().ok_or_else(|| {
        tracing::warn!(
            route,
            tenant = %hex(&keys.identity.pid),
            "csm: онлайн-ключ не настроен ({}), подпись невозможна",
            super::keys::ENV_ONLINE_KEY
        );
        StatusCode::SERVICE_UNAVAILABLE
    })
}

/// Подписка по локатору. Промах один раз пробует дозаполнить локаторы
/// подписок, у которых их ещё нет (строки появляются лениво, см.
/// `catalog_store`), затем отвечает 404, неотличимым от любого другого.
async fn locate(
    state: &AppState,
    identity: &TenantIdentity,
    loc: &Locator,
    route: &'static str,
) -> Result<store::LocatedSubscription, StatusCode> {
    let lookup = |pool| store::find_by_locator(pool, loc);
    let found = lookup(&state.pool).await.map_err(|e| {
        tracing::error!(route, tenant = %hex(&identity.pid), error = %e, "csm: поиск локатора");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if let Some(sub) = found {
        return Ok(sub);
    }
    // Ленивое дозаполнение остаётся на теневую фазу: подписка, созданная после
    // старта, получает локатор при первом промахе. Ошибка секрета не глотается:
    // опечатка оператора превращала бы каждый промах в молчаливый 404.
    let secret = match super::loc_secret_from_env() {
        Ok(secret) => secret,
        Err(e) => {
            tracing::error!(route, error = %e, "csm: секрет локатора не разбирается");
            None
        }
    };
    if let Some(secret) = secret {
        match store::backfill_locators(&state.pool, &secret).await {
            Ok(Some(added)) if added > 0 => {
                if let Ok(Some(sub)) = lookup(&state.pool).await {
                    return Ok(sub);
                }
            }
            Ok(_) => {}
            Err(e) => tracing::error!(route, error = %e, "csm: дозаполнение локаторов"),
        }
    }
    Err(StatusCode::NOT_FOUND)
}

// ---------------------------------------------------------------- /sub/k1

/// `GET /sub/k1`, ключевой документ, якорь доверия тенанта.
///
/// Панель НЕ подписывает его: корневой приватный ключ живёт офлайн у оператора,
/// и в этом весь смысл разделения ролей. Взлом работающей панели даёт
/// онлайн-ключ, но не даёт подменить якорь. Поэтому маршрут отдаёт то, что
/// оператор подписал у себя и импортировал, а если импорта не было, честно
/// отвечает 503 вместо документа, подписанного не той ролью.
async fn key_document(State(state): State<AppState>) -> Response {
    serve_root_document(&state, DocType::Key).await
}

/// Отдаёт последнюю версию корневого документа заданного типа.
async fn serve_root_document(state: &AppState, doc_type: DocType) -> Response {
    match latest_root_document(&state.pool, doc_type).await {
        Ok(Some(frame)) => frame_response(frame, KEY_CACHE_CONTROL),
        Ok(None) => refuse(StatusCode::SERVICE_UNAVAILABLE),
        Err(e) => {
            tracing::error!(error = %e, doc_type = doc_type.as_u8(), "csm: чтение корневого документа");
            refuse(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Последняя импортированная версия документа.
async fn latest_root_document(
    pool: &sqlx::PgPool,
    doc_type: DocType,
) -> anyhow::Result<Option<Vec<u8>>> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT frame FROM csm_root_documents WHERE doc_type = $1 ORDER BY ver DESC LIMIT 1",
    )
    .bind(doc_type.as_u8() as i16)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(frame,)| frame))
}

// ---------------------------------------------------------------- /sub/c1

/// `GET /sub/c1/{cat_id}/{i}`, одна подписанная часть каталога.
///
/// Допуск по `X-CSM-Loc`, чтобы путь оставался ключом кэша. Чужой тир и
/// несуществующий каталог отвечают одинаковым 404: различие превратило бы
/// маршрут в оракул принадлежности `cat_id` тирам (13.5).
async fn catalog_chunk(
    State(state): State<AppState>,
    Path((cat_id, index)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    match chunk_inner(&state, &cat_id, &index, &headers).await {
        Ok(r) => r,
        Err(status) => refuse(status),
    }
}

async fn chunk_inner(
    state: &AppState,
    cat_id: &str,
    index: &str,
    headers: &HeaderMap,
) -> Result<Response, StatusCode> {
    const ROUTE: &str = "/sub/c1";
    let loc = headers
        .get(HEADER_LOC)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| Locator::parse(s).ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let cid = store::parse_cat_id(cat_id).ok_or(StatusCode::BAD_REQUEST)?;
    let i = store::parse_chunk_index(index).ok_or(StatusCode::BAD_REQUEST)?;

    rate_limit(state, "c1", "loc", &locator_subject(&loc), C1_PER_LOC).await?;
    rate_limit(state, "c1", "ip", &client_ip(headers), C1_PER_IP).await?;

    let identity = load_identity(state, ROUTE).await?;
    let sub = locate(state, &identity, &loc, ROUTE).await?;
    // Локатор переживает истечение и блокировку, а материал узлов получает
    // только подписка, которой можно подключаться. Отказ неотличим от
    // неизвестного cat_id (13.5).
    if !sub.may_read_chunks(chrono::Utc::now().timestamp()) {
        return Err(StatusCode::NOT_FOUND);
    }
    let tier = store::tier_of_plan(sub.plan_id).map_err(|e| {
        tracing::warn!(route = ROUTE, tenant = %hex(&identity.pid), error = %e, "csm: тир подписки");
        StatusCode::NOT_FOUND
    })?;

    // Валидатор известен до чтения базы: условный GET совпавшего тега не
    // стоит ни одного запроса к хранилищу. Строится из канонического
    // написания cid, а не из пути: читатель Crockford принимает `O` за `0`.
    // 304 здесь уходит и на часть, которой у тира подписки нет: вызывающий
    // узнаёт только то, что сам прислал в If-None-Match, а принадлежность
    // cat_id тиру он уже подтвердил, когда получил этот ETag с телом.
    let etag = store::chunk_etag(&csm::catalog::base32_crockford(&cid), i);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
    {
        return Ok(not_modified(&etag));
    }

    let frame = store::CatalogStore::new(&state.pool)
        .chunk(tier, &cid, i)
        .await
        .map_err(|e| {
            tracing::error!(route = ROUTE, tenant = %hex(&identity.pid), error = %e, "csm: чтение части");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !store::chunk_fits(&frame) {
        tracing::error!(
            route = ROUTE,
            tenant = %hex(&identity.pid),
            tier,
            len = frame.len(),
            "csm: кадр части выше CHUNK_RESP_MAX, не отдаётся"
        );
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    Ok(chunk_response(frame, &etag))
}

// ---------------------------------------------------------------- /sub/m1

/// Параметры `?n=&v=&d=` (13.2). Все обязательны; разбираются вручную, чтобы
/// отсутствие любого давало пустой 400, а не текст извлекателя axum.
#[derive(Debug, Deserialize)]
struct DirectiveQuery {
    n: Option<String>,
    v: Option<String>,
    d: Option<String>,
}

/// `GET /sub/m1/{loc}?n=&v=&d=`, директива для устройства.
///
/// Здесь нет `track_access` и нет учёта устройств по IP (13.3): каждая
/// ступень лестницы клиента меняет видимый адрес, и счётчик по IP сделал бы
/// лестницу самоубийственной.
async fn directive(
    State(state): State<AppState>,
    Path(loc): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    match directive_inner(&state, &loc, query.as_deref().unwrap_or(""), &headers).await {
        Ok(r) => r,
        Err(status) => refuse(status),
    }
}

async fn directive_inner(
    state: &AppState,
    loc: &str,
    query: &str,
    headers: &HeaderMap,
) -> Result<Response, StatusCode> {
    const ROUTE: &str = "/sub/m1";
    let loc = Locator::parse(loc).map_err(|_| StatusCode::BAD_REQUEST)?;
    let q: DirectiveQuery =
        serde_urlencoded::from_str(query).map_err(|_| StatusCode::BAD_REQUEST)?;
    let nonce =
        Nonce::from_query(q.n.as_deref().unwrap_or("")).map_err(|_| StatusCode::BAD_REQUEST)?;
    let dtp = DeviceThumbprint::from_query(q.d.as_deref().unwrap_or(""))
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let client_ver: u64 =
        q.v.as_deref()
            .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|v| v.parse().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;

    rate_limit(state, "m1", "loc", &locator_subject(&loc), M1_PER_LOC).await?;
    rate_limit(state, "m1", "ip", &client_ip(headers), M1_PER_IP).await?;

    let keys = load_signer(state, ROUTE).await?;
    let online = online_key(&keys, ROUTE)?;
    let pid = keys.identity.pid;
    let tenant = hex(&pid);

    let sub = locate(state, &keys.identity, &loc, ROUTE).await?;
    let tier = store::tier_of_plan(sub.plan_id).map_err(|e| {
        tracing::error!(route = ROUTE, tenant = %tenant, error = %e, "csm: подписка вне тиров");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    // Каталог тира: чтение, если содержимое не менялось, подпись, если
    // менялось. Тир без единого выхода это отказ подписи, и клиент получает
    // 503, а не каталог, по которому нельзя подключиться.
    let catalog = store::ensure_tier(state, online, pid, tier)
        .await
        .map_err(|e| {
            tracing::error!(route = ROUTE, tenant = %tenant, tier, error = %e, "csm: каталог тира");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    let ver = store::next_directive_ver(&state.pool, sub.id)
        .await
        .map_err(|e| {
            tracing::error!(route = ROUTE, tenant = %tenant, error = %e, "csm: версия директивы");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if client_ver >= ver {
        // Клиент держит версию, которую панель ещё не выдавала: либо база
        // откатывалась, либо кто-то подделал отметку. Обслуживание продолжается,
        // а факт остаётся в журнале для оператора (01-DECISION.md 5.4.6).
        tracing::warn!(
            route = ROUTE,
            tenant = %tenant,
            subscription = sub.id,
            client_ver,
            ver,
            "csm: клиент сообщает версию выше выданной"
        );
    }

    let now = chrono::Utc::now().timestamp();
    let (status, reason) = store::classify(&sub.status_facts(now));
    let selection = store::selection_for(&catalog.model, sub.node_id, sub.relay_country.as_deref());
    let traffic = caramba_shared::csm::directive::Traffic {
        up: 0,
        down: sub.used_traffic.max(0) as u64,
        total: sub.limit_bytes().max(0) as u64,
        expires: sub.expires_at.timestamp().max(0) as u64,
    };

    let directive = store::assemble_directive(store::DirectiveInputs {
        pid,
        ver,
        iat: now as u64,
        nonce,
        dtp,
        status,
        reason,
        catalog: &catalog.stored,
        cap: catalog.model.cap,
        selection,
        ttl: catalog.model.ttl,
        locator: loc,
        traffic: Some(traffic),
    })
    .map_err(|e| {
        tracing::error!(route = ROUTE, tenant = %tenant, error = %e, "csm: сборка директивы");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Запечатывания (0x06) пока нет: у панели нет ни реестра ключей
    // устройств, ни HPKE, и бит SEALED_DIRECTIVES в cap не выставлен. Клиент
    // получает подписанный кадр 0x03 с набивкой на сетке. Это заявленное
    // отклонение от 13.2: с чистым битом 1 клиент берёт директиву только со
    // ступени R1 (02-SPEC.md 6.3) и не считает ответ ошибкой. `d=` при этом
    // разбирается и эхом уходит в `dtp`, но не сверяется с реестром, которого
    // нет; 404 за незарегистрированное устройство появится вместе с ним.
    let bucket = store::draw_bucket(catalog.model.pad_buckets);
    let frame = directive
        .sign(std::slice::from_ref(online), bucket)
        .map_err(|e| {
            tracing::error!(route = ROUTE, tenant = %tenant, error = %e, "csm: подпись директивы");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(frame_response(frame, NO_STORE))
}

// ---------------------------------------------------------------- импорт корневых документов
//
// Вход офлайн-церемонии. Команда CLI, которая его вызывает, ещё не подключена,
// поэтому путь покрыт тестами, но не продакшн-кодом.

/// Импортирует документ, подписанный офлайн корневым ключом.
///
/// Проверяет только то, что панель обязана проверить, чтобы не отдавать заведомо
/// нерабочий кадр: магию, тип, правило точной длины и совпадение `pid` с
/// личностью тенанта. Подпись проверяет клиент; панель не является доверенной
/// стороной для собственного якоря.
#[allow(dead_code)]
pub fn validate_root_frame(
    frame: &[u8],
    expected: DocType,
    identity_pid: &[u8; 8],
) -> anyhow::Result<()> {
    use anyhow::{anyhow, ensure};

    ensure!(frame.len() > 8, "кадр короче минимального");
    ensure!(frame[..4] == csm::MAGIC, "не кадр CSM/1: магия не совпала");
    ensure!(
        frame[4] == expected.as_u8(),
        "тип документа {} вместо ожидаемого {}",
        frame[4],
        expected.as_u8()
    );

    let payload_len = u16::from_be_bytes([frame[5], frame[6]]) as usize;
    let nsigs_off = 7 + payload_len;
    ensure!(frame.len() > nsigs_off, "длина payload выходит за кадр");
    let nsigs = frame[nsigs_off] as usize;
    ensure!((1..=4).contains(&nsigs), "число подписей вне диапазона");
    let expected_len = 7 + payload_len + 1 + 76 * nsigs;
    ensure!(
        frame.len() == expected_len,
        "правило точной длины нарушено: {} вместо {}",
        frame.len(),
        expected_len
    );

    // pid лежит в конверте под ключом 2 как bstr(8): `02 48 <8 байт>`.
    let payload = &frame[7..7 + payload_len];
    let pid_at = payload
        .windows(2)
        .position(|w| w == [0x02, 0x48])
        .ok_or_else(|| anyhow!("в конверте нет поля pid"))?;
    let pid = &payload[pid_at + 2..pid_at + 10];
    ensure!(pid == identity_pid, "документ подписан для другого тенанта");

    Ok(())
}

/// Сохраняет проверенный корневой документ. Возвращает hex sha256 кадра.
///
/// Это вход офлайн-церемонии: оператор подписал документ у себя, панель его
/// принимает, проверяет форму и отдаёт клиентам как есть.
#[allow(dead_code)]
pub async fn import_root_document(
    pool: &sqlx::PgPool,
    doc_type: DocType,
    frame: &[u8],
    identity_pid: &[u8; 8],
) -> anyhow::Result<String> {
    validate_root_frame(frame, doc_type, identity_pid)?;
    let ver = envelope_version(frame)?;
    let hash = super::hex(&csm::frame_digest(frame));

    sqlx::query(
        "INSERT INTO csm_root_documents (doc_type, ver, frame, frame_hash) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (doc_type, ver) DO UPDATE \
           SET frame = EXCLUDED.frame, frame_hash = EXCLUDED.frame_hash, imported_at = NOW()",
    )
    .bind(doc_type.as_u8() as i16)
    .bind(ver as i64)
    .bind(frame)
    .bind(&hash)
    .execute(pool)
    .await?;

    Ok(hash)
}

/// Достаёт `ver` из конверта: ключ 3, беззнаковое целое кратчайшей формы.
#[allow(dead_code)]
fn envelope_version(frame: &[u8]) -> anyhow::Result<u64> {
    use anyhow::{anyhow, ensure};
    let payload_len = u16::from_be_bytes([frame[5], frame[6]]) as usize;
    let payload = &frame[7..7 + payload_len];
    // Конверт фиксирован: a7.. 01 01 02 48 <8> 03 <ver>. Ищем ключ 3 сразу за pid.
    let pid_at = payload
        .windows(2)
        .position(|w| w == [0x02, 0x48])
        .ok_or_else(|| anyhow!("в конверте нет поля pid"))?;
    let at = pid_at + 10;
    ensure!(
        payload.len() > at + 1 && payload[at] == 0x03,
        "в конверте нет поля ver"
    );
    let head = payload[at + 1];
    Ok(match head {
        0x00..=0x17 => head as u64,
        0x18 => payload[at + 2] as u64,
        0x19 => u16::from_be_bytes([payload[at + 2], payload[at + 3]]) as u64,
        0x1a => u32::from_be_bytes([
            payload[at + 2],
            payload[at + 3],
            payload[at + 4],
            payload[at + 5],
        ]) as u64,
        other => return Err(anyhow!("неподдерживаемая голова ver: {other:#04x}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn corpus(name: &str) -> Option<Vec<u8>> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/caramba-client/docs/protocol/05-TEST-VECTORS")
            .join(name);
        std::fs::read(p).ok()
    }

    /// pid тенанта корпуса.
    const CORPUS_PID: [u8; 8] = [0x22, 0x6e, 0x8a, 0x20, 0xf6, 0x99, 0xb9, 0x64];

    #[test]
    fn accepts_the_reference_key_document() {
        let Some(frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        validate_root_frame(&frame, DocType::Key, &CORPUS_PID).expect("эталонный кадр принят");
        assert_eq!(envelope_version(&frame).unwrap(), 1);
    }

    #[test]
    fn rejects_a_trailing_byte() {
        let Some(mut frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        frame.push(0x00);
        let err = validate_root_frame(&frame, DocType::Key, &CORPUS_PID).unwrap_err();
        assert!(err.to_string().contains("точной длины"), "{err}");
    }

    #[test]
    fn rejects_the_wrong_document_type() {
        let Some(frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        assert!(validate_root_frame(&frame, DocType::Bootstrap, &CORPUS_PID).is_err());
    }

    #[test]
    fn rejects_another_tenant() {
        let Some(frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        let err = validate_root_frame(&frame, DocType::Key, &[0u8; 8]).unwrap_err();
        assert!(err.to_string().contains("другого тенанта"), "{err}");
    }

    #[test]
    fn accepts_the_reference_bootstrap_blob() {
        let Some(frame) = corpus("bin/positive/b1_wire_8_5.bin") else {
            return;
        };
        validate_root_frame(&frame, DocType::Bootstrap, &CORPUS_PID).expect("блоб принят");
    }

    fn header_names(r: &Response) -> Vec<String> {
        let mut names: Vec<String> = r.headers().keys().map(|k| k.as_str().to_string()).collect();
        names.sort();
        names
    }

    #[test]
    fn chunk_response_carries_exactly_the_spec_header_set() {
        let etag = store::chunk_etag("XDE36CGS838HG4W4", 0);
        let r = chunk_response(vec![0xca, 0xfe], &etag);
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            header_names(&r),
            ["cache-control", "content-type", "etag", "vary"]
        );
        assert_eq!(
            r.headers()[header::CONTENT_TYPE],
            "application/vnd.caramba.csm1"
        );
        assert_eq!(
            r.headers()[header::CACHE_CONTROL],
            "private, max-age=86400, immutable"
        );
        assert_eq!(r.headers()[header::ETAG], "\"XDE36CGS838HG4W4-0\"");
        assert_eq!(r.headers()[header::VARY], "X-CSM-Loc");
        assert!(r.headers().get(header::CONTENT_ENCODING).is_none());

        let nm = not_modified(&etag);
        assert_eq!(nm.status(), StatusCode::NOT_MODIFIED);
        assert!(nm.headers().get(header::CONTENT_TYPE).is_none());
    }

    #[test]
    fn directive_and_key_responses_carry_only_type_and_cache_control() {
        let d = frame_response(vec![0x01], NO_STORE);
        assert_eq!(header_names(&d), ["cache-control", "content-type"]);
        assert_eq!(d.headers()[header::CACHE_CONTROL], "no-store");
        let k = frame_response(vec![0x01], KEY_CACHE_CONTROL);
        assert_eq!(k.headers()[header::CACHE_CONTROL], "public, max-age=300");
    }

    #[test]
    fn refusals_have_an_empty_body_and_no_content_type() {
        let r = refuse(StatusCode::NOT_FOUND);
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert!(r.headers().get(header::CONTENT_TYPE).is_none());
    }

    /// Собранный сервис панели: реальные слои `main.rs` на роутере-заглушке
    /// и маршруты CSM, отвечающие теми же строителями ответов, что и
    /// настоящие обработчики. Запрос несёт `Accept-Encoding: gzip, br`,
    /// чтобы прицепившийся слой сжатия провалил проверку, а не прошёл её.
    #[tokio::test]
    async fn csm_routes_escape_compression_and_the_constant_headers() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let legacy = Router::new().route("/sub/legacy-uuid", get(|| async { "x".repeat(4096) }));
        let layered = crate::panel_layers(legacy);
        let csm = Router::new()
            .route(
                "/sub/k1",
                get(|| async { frame_response(vec![0u8; 512], KEY_CACHE_CONTROL) }),
            )
            .route(
                "/sub/c1/{cat_id}/{i}",
                get(|| async {
                    let etag = store::chunk_etag("XDE36CGS838HG4W4", 0);
                    chunk_response(vec![0u8; 3072], &etag)
                }),
            )
            .route(
                "/sub/m1/{loc}",
                get(|| async { frame_response(vec![0u8; 1024], NO_STORE) }),
            );
        let app = mount(layered, csm);

        let request = |path: &str| {
            Request::builder()
                .uri(path)
                .header(header::ACCEPT_ENCODING, "gzip, br")
                .body(Body::empty())
                .unwrap()
        };

        // Слои настоящие: легаси-маршрут сжат и проштампован.
        let r = app
            .clone()
            .oneshot(request("/sub/legacy-uuid"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers().get(header::CONTENT_ENCODING).is_some());
        assert!(r.headers().get(header::X_FRAME_OPTIONS).is_some());

        // Маршруты CSM: ровно набор 13.4, без Content-Encoding и без
        // пяти постоянных заголовков.
        // `Content-Length` это длина кадра, `Date` ставит сервер при отдаче.
        let cases = [
            (
                "/sub/k1",
                512,
                vec!["cache-control", "content-length", "content-type"],
            ),
            (
                "/sub/c1/XDE36CGS838HG4W4/0",
                3072,
                vec![
                    "cache-control",
                    "content-length",
                    "content-type",
                    "etag",
                    "vary",
                ],
            ),
            (
                "/sub/m1/EA3B8SKCY6VBWASE7AM1X48Y",
                1024,
                vec!["cache-control", "content-length", "content-type"],
            ),
        ];
        for (path, frame_len, want) in cases {
            let r = app.clone().oneshot(request(path)).await.unwrap();
            assert_eq!(r.status(), StatusCode::OK, "{path}");
            assert_eq!(header_names(&r), want, "{path}: набор заголовков");
            assert_eq!(
                r.headers()[header::CONTENT_TYPE],
                CSM_CONTENT_TYPE,
                "{path}"
            );
            assert_eq!(
                r.headers()[header::CONTENT_LENGTH],
                frame_len.to_string(),
                "{path}: длина на проводе равна длине кадра"
            );
        }
    }

    #[test]
    fn locator_subject_is_a_hash_not_the_credential() {
        let loc = Locator::parse("EA3B8SKCY6VBWASE7AM1X48Y").unwrap();
        let subject = locator_subject(&loc);
        assert_eq!(subject.len(), 32);
        assert!(!subject.contains(loc.as_str()));
        assert_eq!(subject, locator_subject(&loc));
    }

    #[test]
    fn client_ip_prefers_the_edge_header() {
        let mut h = HeaderMap::new();
        assert_eq!(client_ip(&h), "0.0.0.0");
        h.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 10.0.0.1"),
        );
        assert_eq!(client_ip(&h), "203.0.113.9");
        h.insert("cf-connecting-ip", HeaderValue::from_static("198.51.100.4"));
        assert_eq!(client_ip(&h), "198.51.100.4");
    }
}

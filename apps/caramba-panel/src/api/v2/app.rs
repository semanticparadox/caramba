//! JWT-защищённые эндпоинты standalone-приложения.
//!
//! Все хендлеры здесь требуют валидный access-токен (middleware
//! `app_auth::require_app_jwt` кладёт `AuthUser` в extensions). Отдаём профиль,
//! данные подписки (включая готовый URL mihomo/clash-конфига, который тянет
//! Go-ядро) и список серверов.

use crate::AppState;
use crate::api::v2::app_auth::AuthUser;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Serialize;
use sqlx::Row;

/// Определяет базовый URL для ссылок подписки (subscription_domain → panel_url →
/// Host-заголовок). Логика дублирует api/client.rs::resolve_subscription_base_url,
/// но локальна, чтобы не делать ту функцию публичной.
///
/// `client_cc` — страна, которую панель увидела у ЭТОГО клиента (`None` —
/// не определилась). От неё зависит первое звено: домен зеркала подписки
/// выдаётся только тем, кого зеркало обслуживает
/// (см. `subscription::mirror_serves_country`). Всем остальным выдаётся адрес
/// панели, и их клиент забирает конфиг одним запросом — без 308 и без крюка
/// через страну зеркала. Это и есть то, ради чего 308 перестал быть безусловным:
/// убрать не только редирект, но и повод для него.
async fn resolve_base_url(
    state: &AppState,
    headers: &HeaderMap,
    client_cc: Option<&str>,
) -> String {
    let sub_domain = state
        .settings
        .get_or_default("subscription_domain", "")
        .await;
    let use_mirror = !sub_domain.is_empty()
        && crate::subscription::mirror_serves_country(state, client_cc).await;
    let base_domain = if use_mirror {
        sub_domain
    } else {
        let panel = state.settings.get_or_default("panel_url", "").await;
        if !panel.is_empty() {
            panel
        } else if let Some(host) = headers.get("host").and_then(|h| h.to_str().ok()) {
            host.to_string()
        } else {
            std::env::var("PANEL_URL").unwrap_or_else(|_| "localhost".to_string())
        }
    };

    if base_domain.starts_with("http") {
        base_domain
    } else {
        let proto = if base_domain.contains("localhost") || base_domain.contains("127.0.0.1") {
            "http"
        } else {
            "https"
        };
        format!("{}://{}", proto, base_domain)
    }
}

/// GET /api/v2/app/me — профиль пользователя: баланс, кол-во активных подписок,
/// имя текущего плана.
pub async fn get_me(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let row = sqlx::query(
        "SELECT id, tg_id, email, username, full_name, balance, referral_code, email_verified, auth_provider \
         FROM users WHERE id = $1",
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let r = match row {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, "User not found").into_response(),
    };

    let balance: i64 = r.try_get("balance").unwrap_or(0);

    // 'throttled' — временная суточная блокировка бесплатного плана; для
    // пользователя такая подписка всё ещё «его тариф», а не отсутствие оного.
    let active_subs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM subscriptions WHERE user_id = $1 AND status IN ('active', 'throttled')",
    )
    .bind(auth.user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    // Имя текущего плана (самая поздняя по сроку активная подписка).
    let plan_name: Option<String> = sqlx::query_scalar(
        "SELECT p.name FROM subscriptions s JOIN plans p ON s.plan_id = p.id \
         WHERE s.user_id = $1 AND s.status IN ('active', 'throttled') \
         ORDER BY s.expires_at DESC LIMIT 1",
    )
    .bind(auth.user_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    Json(serde_json::json!({
        "id": auth.user_id,
        "tg_id": r.try_get::<Option<i64>, _>("tg_id").ok().flatten(),
        "email": r.try_get::<Option<String>, _>("email").ok().flatten(),
        "username": r.try_get::<Option<String>, _>("username").ok().flatten(),
        "full_name": r.try_get::<Option<String>, _>("full_name").ok().flatten(),
        "balance": balance as f64 / 100.0,
        "balance_cents": balance,
        "referral_code": r.try_get::<Option<String>, _>("referral_code").ok().flatten(),
        "email_verified": r.try_get::<Option<bool>, _>("email_verified").ok().flatten().unwrap_or(false),
        "auth_provider": r.try_get::<Option<String>, _>("auth_provider").ok().flatten(),
        "active_subscriptions": active_subs,
        "plan_name": plan_name,
    }))
    .into_response()
}

/// GET /api/v2/app/subscription — данные подписки и готовые URL конфигов.
///
/// Возвращает subscription_uuid + набор URL для разных клиентов. Ключевой для
/// Go-ядра — `clash_url` (mihomo тянет именно его, amnezia-wg уже в конфиге).
/// Ссылки на оплату для этой установки.
///
/// Читаются из настроек, а не зашиты: имя бота и короткое имя мини-аппа —
/// свойство инсталляции, и любая вторая копия этих строк однажды разъедется с
/// первой. `None` — бот не настроен; тогда приложение просто не рисует кнопку,
/// что честнее мёртвой ссылки.
async fn pay_links(state: &AppState) -> Option<crate::subscription::access::PayLinks> {
    let bot = state.settings.get_or_default("bot_username", "").await;
    let short = state
        .settings
        .get_or_default("mini_app_short_name", "")
        .await;
    crate::subscription::access::pay_links(&bot, &short)
}

pub async fn get_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    let subs = match state
        .subscription_service
        .get_user_subscriptions(auth.user_id)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(err = %e, "app: failed to fetch subscriptions");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch subscription",
            )
                .into_response();
        }
    };

    // Активная, иначе задушенная за трафик, иначе первая доступная.
    //
    // Средняя ступень появилась не для красоты: без неё пользователь, у
    // которого бесплатная подписка ушла в суточную блокировку, а рядом лежит
    // старая закрытая, получал в приложении карточку СТАРОЙ — с чужим тарифом,
    // чужим сроком и совершенно посторонним объяснением, почему нет связи.
    // Подписка, которая станет рабочей сама после полуночи, — это та, о которой
    // человек спрашивает.
    let sub = subs
        .iter()
        .find(|s| s.sub.status == "active")
        .or_else(|| subs.iter().find(|s| s.sub.status == "throttled"))
        .or_else(|| subs.first());

    let sub = match sub {
        Some(s) => s,
        None => return (StatusCode::NOT_FOUND, "No subscription found").into_response(),
    };

    // Та же цепочка, что и в `subscription::subscription_handler`: заголовок
    // обратного прокси, иначе GeoIP по адресу клиента. Одна функция на оба
    // пути — разъехавшись, они выдали бы клиенту URL одного домена, а тело
    // отдали бы с другого.
    let client_ip = crate::subscription::extract_client_ip(&headers);
    let header_cc = headers
        .get("x-country-code")
        .or_else(|| headers.get("cf-ipcountry"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_uppercase())
        .filter(|cc| cc.len() == 2 && cc != "XX" && cc != "T1");
    let client_cc =
        crate::subscription::resolve_client_country(&state, &client_ip, header_cc.as_deref()).await;

    let base_url = resolve_base_url(&state, &headers, client_cc.as_deref()).await;
    let uuid = &sub.sub.subscription_uuid;

    let used_gb = sub.sub.used_traffic as f64 / 1024.0 / 1024.0 / 1024.0;
    let days_left = (sub.sub.expires_at - chrono::Utc::now()).num_days().max(0);

    // Тот же потолок, что и в энфорсменте: лимит тарифа + бонусный трафик.
    let bonus_traffic_mb = crate::services::bonus_traffic::balance_mb(&state.pool, auth.user_id)
        .await
        .unwrap_or(0);

    // `SubscriptionWithDetails` несёт из плана только traffic_limit_gb, а на
    // бесплатном плане энфорсмент считает совсем по другой колонке
    // (daily_traffic_mb). Без этих двух флагов потолок пришлось бы угадывать —
    // и раньше он угадывался как «план платный», из-за чего бесплатный
    // пользователь, задушенный на 200 МБ, видел в приложении 2% от 10 ГБ.
    let (is_free, daily_traffic_mb): (bool, i32) = sqlx::query_as(
        "SELECT COALESCE(is_free, FALSE), COALESCE(daily_traffic_mb, 0) FROM plans WHERE id = $1",
    )
    .bind(sub.sub.plan_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None)
    .unwrap_or((false, 0));

    let traffic_limit_gb = sub.traffic_limit_gb.unwrap_or(0);
    let traffic_limit_bytes = crate::services::bonus_traffic::plan_quota_limit_bytes(
        is_free,
        traffic_limit_gb as i64,
        daily_traffic_mb as i64,
        bonus_traffic_mb,
    );
    // Период, за который посчитаны traffic_limit_bytes и used_traffic_bytes:
    // "day" ровно тогда, когда потолок взят из суточной колонки. Приложению
    // это нужно, чтобы рисовать суточный счётчик, а не бессмысленный «всего».
    let quota_period = if is_free && traffic_limit_gb > 0 && daily_traffic_mb > 0 {
        "day"
    } else {
        "total"
    };

    // Состояние доступа: одна структура, по которой приложение решает, пускать
    // ли в подключение, и один текст, который оно вправе показать как есть.
    //
    // Всё, что ниже в этом JSON, — сырые числа: приложение обязано было само
    // сложить из них вывод «трафик кончился, приходи после полуночи», и не
    // складывало, потому что не знало ни про суточный период, ни про то, что
    // пополнение вычитает норму, а не обнуляет расход. Вывод теперь делает
    // панель — тем же кодом, которым она объясняет отказ на `/sub/{uuid}`, так
    // что экран приложения и заголовок ответа физически не могут разойтись.
    let (device_used, device_limit): (i64, i32) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM subscription_device_leases sdl \
                 WHERE sdl.subscription_id = $1 \
                   AND sdl.last_seen_at > NOW() - INTERVAL '15 minutes' \
                   AND sdl.last_ip <> '0.0.0.0'), \
                COALESCE((SELECT device_limit FROM plans WHERE id = $2), 0)",
    )
    .bind(sub.sub.id)
    .bind(sub.sub.plan_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None)
    .unwrap_or((0, 0));

    let banned: bool =
        sqlx::query_scalar("SELECT COALESCE(is_banned, FALSE) FROM users WHERE id = $1")
            .bind(auth.user_id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None)
            .unwrap_or(false);

    let access = crate::subscription::access::compute(
        &crate::subscription::access::AccessFacts {
            status: sub.sub.status.clone(),
            banned,
            expires_at: sub.sub.expires_at.timestamp(),
            used_bytes: (sub.sub.used_traffic as i64).max(0),
            limit_bytes: traffic_limit_bytes,
            is_free,
            daily_traffic_mb: daily_traffic_mb as i64,
            device_used,
            device_limit: device_limit as i64,
            now: chrono::Utc::now().timestamp(),
        },
        pay_links(&state).await,
    );

    Json(serde_json::json!({
        "id": sub.sub.id,
        "subscription_uuid": uuid,
        "plan_name": sub.plan_name,
        // Сырой статус базы. Остаётся ради уже выпущенных клиентов и НЕ является
        // тем, что показывают человеку: слово `throttled` внутреннее, объяснение
        // живёт в `access`.
        "status": sub.sub.status,
        "access": access,
        // На суточном плане это расход, ещё не прощённый суточным пополнением
        // (monitoring::daily_traffic_topup вычитает норму с полом 0), то есть
        // фактически «сегодня»; на остальных — расход за весь срок подписки.
        // Что именно — говорит quota_period.
        "used_traffic_bytes": sub.sub.used_traffic,
        "used_traffic_gb": format!("{:.2}", used_gb),
        // Сырая колонка плана. На бесплатном плане она НЕ является потолком —
        // клиент обязан считать по traffic_limit_bytes / quota_period.
        "traffic_limit_gb": sub.traffic_limit_gb,
        "traffic_limit_bytes": traffic_limit_bytes,
        "quota_period": quota_period,
        "is_free": is_free,
        // Суточная норма плана в МБ (0 — суточной нормы нет). Отдаём сырую
        // колонку отдельно от потолка: в traffic_limit_bytes уже подмешан
        // бонусный трафик, а нарисовать «200 МБ в сутки» надо без него.
        "daily_traffic_mb": daily_traffic_mb,
        "bonus_traffic_mb": bonus_traffic_mb,
        "expires_at": sub.sub.expires_at.to_rfc3339(),
        "days_left": days_left,
        // URL'ы конфигов. Go-ядро (mihomo) использует clash_url.
        "clash_url": format!("{}/sub/{}?client=clash", base_url, uuid),
        "config_url": format!("{}/sub/{}?client=clash", base_url, uuid),
        "singbox_url": format!("{}/sub/{}?client=singbox", base_url, uuid),
        "v2ray_url": format!("{}/sub/{}?client=v2ray", base_url, uuid),
        "subscription_url": format!("{}/sub/{}", base_url, uuid),
        // Страна, которую панель увидела у этого клиента; `null` — не
        // определилась (нет MaxMind-базы, не ответил внешний сервис, приватный
        // адрес). Клиент своей страны не знает ниоткуда больше: геобазы он не
        // носит, а спрашивать её у пользователя значит спрашивать о том, что
        // сервер и так видит.
        //
        // Нужна ему для двух решений, у которых сегодня зашита Россия: пресет
        // маршрутизации по умолчанию и домашний резолвер. `null` обязан
        // оставаться `null` — подставленная сюда страна увела бы человека в
        // чужой национальный режим молча.
        "client_country": client_cc,
    }))
    .into_response()
}

#[derive(Serialize)]
struct AppServer {
    id: i64,
    name: String,
    country_code: Option<String>,
    /// Флаг ДЛЯ ПОКАЗА. Считается алгоритмически из ISO-2 ([`country_flag`]),
    /// поэтому знает любую страну. Флаг внутри `inbounds[].proxy_name` берётся
    /// из другой, ручной таблицы генератора Clash и для страны, которой в ней
    /// нет, даёт `🌐`. Расхождение намеренно не сглажено: `proxy_name` обязан
    /// побайтово совпасть с телом конфига, а показывать `🌐` вместо реального
    /// флага — деградация без причины. Сегодня DE/CA есть в обеих таблицах.
    flag: String,
    latency_ms: Option<i32>,
    load_pct: f64,
    status: String,
    /// Релэй, к которому узел привязан колонкой `nodes.relay_id`. Не выбор:
    /// выбор входа живёт в `GET /relays` + `?relay_country=`. Здесь — контекст,
    /// объясняющий суффикс `↪` в именах прокси этого узла.
    via_relay: Option<AppRelayHop>,
    /// Инбаунды узла — то, из чего состоит пикер протокола. `null` означает
    /// «панель не смогла их прочитать», и тогда причина лежит в
    /// `inbounds_error`; пустой массив означает «у узла нет ни одного
    /// включённого инбаунда». Это разные вещи, и клиент обязан их различать.
    inbounds: Option<Vec<AppInbound>>,
    inbounds_error: Option<String>,
}

/// Хоп через релэй, каким его видит подписка этого узла.
#[derive(Serialize)]
struct AppRelayHop {
    node_id: i64,
    name: String,
    country_code: Option<String>,
    flag: String,
    /// Строит ли клиентский рендерер настоящую цепочку через этот релэй.
    ///
    /// На mihomo/Clash — нет, и это проверенный факт, а не осторожность:
    /// `generate_clash_config` принимает `_relay_nodes` и не обращается к ним,
    /// ни `dialer-proxy`, ни группы `type: relay` в выпуске нет, а `server:`
    /// у «релэйного» прокси — всё тот же адрес выхода. Суффикс `↪` и группа
    /// `Auto-Relay` это ярлык. Настоящую цепочку через `detour` строит только
    /// генератор sing-box (путь Hiddify/v2rayNG). Поле отдаётся, чтобы
    /// приложение показало вход выключенным С ПРИЧИНОЙ, а не нарисовало
    /// переключатель, не меняющий на проводе ни байта.
    chained_in_config: bool,
}

/// Один включённый инбаунд узла — строка пикера протокола.
///
/// Тройка `protocol` / `network` / `security` разъединена намеренно:
/// `vless/tcp/reality` и `vless/tcp/tls` это РАЗНЫЕ строки пикера, и склеить
/// их в одно слово «vless» значит предложить пользователю выбор, который
/// ничего не выбирает.
#[derive(Serialize)]
struct AppInbound {
    /// `inbounds.id`; `null` у легаси-прокси, который генератор синтезирует из
    /// колонок узла, когда включённых инбаундов нет вовсе.
    id: Option<i64>,
    /// Тег из панели — операторская идентичность строки.
    tag: String,
    protocol: String,
    /// Транспорт: tcp / ws / grpc / httpupgrade / udp / xhttp.
    network: String,
    /// reality / tls / none.
    security: String,
    port: i64,
    /// Короткая подпись из `format_proto_label` — та же, что уходит в имя
    /// прокси в теле конфига.
    label: String,
    /// ТОЧНОЕ имя прокси в теле Clash-конфига, побайтово. Это ключ, которым
    /// приложение связывает строку пикера с выбором в селекторе CARAMBA
    /// (и с `Server.ID`, который отдаёт Go-ядро). `null`, когда прокси в теле
    /// нет — тогда смотри `unavailable_reason`.
    proxy_name: Option<String>,
    available: bool,
    /// Машиночитаемая причина недоступности; `null` у доступных.
    unavailable_reason: Option<&'static str>,
}

/// Эмодзи-флаг по ISO-2 коду страны (без unwrap на данных из БД).
fn country_flag(country: &str) -> String {
    let chars: Vec<char> = country
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    if chars.len() != 2 {
        return "🌐".to_string();
    }
    let offset = 127397u32;
    match (
        char::from_u32(chars[0] as u32 + offset),
        char::from_u32(chars[1] as u32 + offset),
    ) {
        (Some(f), Some(s)) => format!("{}{}", f, s),
        _ => "🌐".to_string(),
    }
}

/// Статус узла в словаре, который понимает приложение: `online | busy | full`.
///
/// Клиент (`ExitLocation` в caramba-client) рисует `full` как «не принимает
/// подключения», а ВСЁ, что не `online` и не `busy` — как «не в сети». Раньше
/// сюда как есть проваливался `nodes.status` из БД, где у всех живых узлов
/// стоит `'active'`: этой строки в словаре нет, поэтому клиент читал каждый
/// узел как offline и список выходов был мёртв целиком. Маппинг обязан жить в
/// панели, а не в клиенте: контракт уже записан на стороне Dart, и этот
/// эндпоинт читают не только Flutter-клиенты.
///
/// Ветки на `'maintenance'` / `'disabled'` здесь намеренно нет: до этой функции
/// доезжают только узлы, прошедшие `status = 'active'` (фильтр в
/// `node_repo::{get_nodes_for_plan, get_active_nodes}` плюс его же повтор в
/// `list_servers`), так что колонка статуса на этом шаге не несёт информации —
/// её несут загрузка и вместимость.
fn server_status(is_full: bool, cpu_pct: f64) -> &'static str {
    if is_full {
        "full"
    } else if cpu_pct > 80.0 {
        "busy"
    } else {
        "online"
    }
}

/// Попадёт ли инбаунд в тело Clash-конфига. `None` — попадёт; `Some(причина)` —
/// нет.
///
/// Это зеркало веток `generate_clash_config`, и оно обязано оставаться
/// зеркалом: строка пикера, которой в теле не соответствует ни один прокси, —
/// это выбор, ведущий в никуда. Ровно так сегодня выглядит `naive` на узле 1:
/// генератор кладёт его имя в группу `Auto-Relay`, но самого прокси не
/// выпускает (у `match inbound.protocol` нет ветки `naive`), и в живом теле
/// висит ссылка `🇩🇪 Naive ↪` на несуществующий узел.
///
/// Причина отдаётся клиенту, а строка не прячется: оператор включил инбаунд в
/// панели и должен видеть, почему он не доехал, а не пустое место.
fn clash_inbound_availability(protocol: &str, network: &str) -> Option<&'static str> {
    // Порядок веток повторяет генератор: сначала два `continue`, потом `match`.
    if protocol.eq_ignore_ascii_case("amneziawg") && !crate::utils::amneziawg_client_enabled() {
        return Some("amneziawg_disabled");
    }
    if matches!(network, "xhttp" | "splithttp") {
        return Some("transport_not_supported_by_clash");
    }
    match protocol.to_ascii_lowercase().as_str() {
        "vless" | "vmess" | "trojan" | "shadowsocks" | "ss" | "hysteria2" | "hy2" | "tuic"
        | "amneziawg" => None,
        _ => Some("protocol_not_emitted_by_clash"),
    }
}

/// GET /api/v2/app/servers — список доступных пользователю exit-серверов
/// вместе с их инбаундами.
///
/// Переиспользует пул узлов из store_service (как api/client.rs::get_active_servers),
/// скрывает relay-инфраструктуру и перегруженные узлы.
///
/// Почему релэи остаются в `GET /relays`, а не приезжают сюда отдельными
/// строками: релэй это не выход. Любой путь генерации конфига вырезает
/// `is_relay` из списка узлов (`subscription.rs`, `filtered_nodes.retain`), так
/// что строка «релэй как сервер» отдавала бы 404 в тот момент, когда
/// приложение попросило бы под неё конфиг. Два пикера и остаются двумя:
/// выход — `/servers` + `?node_id=`, вход — `/relays` + `?relay_country=`.
/// Здесь релэй появляется только как `via_relay` — контекст выбранного узла,
/// доступный на чтение.
/// План, по которому строится СПИСОК серверов приложения.
///
/// Отличается от `subscription_repo::get_active_plan_id_by_user` ровно одним:
/// сюда попадает и подписка, задушенная за суточный трафик. Это намеренно и
/// ограничено показом — раздача узлам и генерация конфига остались на
/// `'active'`. Константа вынесена, чтобы у расширения был тест: вернуть его
/// обратно к одному `'active'` можно только осознанно.
const LISTING_PLAN_SQL: &str = "SELECT plan_id, status FROM subscriptions WHERE user_id = $1 \
       AND status IN ('active', 'throttled') \
     ORDER BY (status = 'active') DESC, id ASC LIMIT 1";

pub async fn list_servers(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    // Список серверов НЕ зависит от квоты — и обязан не зависеть.
    //
    // `store_service::get_user_nodes` ищет план по `status = 'active'`, и для
    // пользователя, задушенного за суточный трафик, возвращал пустой вектор.
    // Приложение показывало пустой экран серверов ровно в тот момент, когда
    // человеку надо было понять, что именно он теряет и за что платит: список
    // исчезал, а причина не появлялась. Вопрос «какие у меня есть сервера» на
    // квоту не завязан, значит и ответ на него завязывать нельзя.
    //
    // Расширяется ТОЛЬКО этот путь — показ. Выдача конфига
    // (`subscription.rs`) и раздача узлам (`subscription_repo`) остаются на
    // `'active'`: задушенный пользователь должен видеть список и не должен
    // получать по нему доступ.
    let plan_row: Option<(i64, String)> = sqlx::query_as(LISTING_PLAN_SQL)
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let nodes: Vec<caramba_db::models::node::Node> = match plan_row.as_ref() {
        Some((id, _)) => state
            .store_service
            .node_repo
            .get_nodes_for_plan(*id)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };

    // Подсказка «этот список показывается, но подключиться по нему сейчас
    // нельзя» — одним заголовком, без второго запроса к `/subscription`.
    // Приложение по нему гасит строки и вешает бейдж сразу, вместе с самим
    // списком. Внутреннего слова в заголовке нет: наружу едет `quota_exceeded`.
    let access_header = plan_row
        .as_ref()
        .filter(|(_, status)| status == "throttled")
        .map(|_| "quota_exceeded");

    let nodes: Vec<caramba_db::models::node::Node> = nodes
        .into_iter()
        .filter(|n| {
            // Прячем relay-узлы и перегруженные машины.
            //
            // `status == "active"` повторяет предикат, который уже применили
            // node_repo::{get_nodes_for_plan, get_active_nodes}. Повтор не
            // лишний: ниже статус узла для приложения собирается ТОЛЬКО из
            // загрузки и вместимости, и это законно ровно потому, что сюда не
            // доезжает ни один неактивный узел. Если фильтр наверху когда-нибудь
            // ослабят, узел в обслуживании просто исчезнет из списка (он и не
            // выбираем), а не притворится живым.
            n.status == "active"
                && !n.is_relay
                && n.last_cpu.unwrap_or(0.0) < 95.0
                && n.last_ram.unwrap_or(0.0) < 98.0
        })
        .collect();

    // Инбаунды и привязку релэя берём ровно тем же вызовом, которым их берёт
    // путь подписки (`subscription.rs`, шаг `get_node_infos_with_relays`).
    // Второй запрос за теми же данными означал бы второй источник истины, а
    // расходиться с телом конфига этому списку нельзя вообще: имя прокси здесь
    // и имя прокси там обязаны быть одной строкой.
    //
    // Отказ этого вызова НЕ пустой список инбаундов: пустой список означал бы
    // «у узла нет протоколов», а это ложь. Отдаём `null` + причину.
    let node_infos = match state
        .subscription_service
        .get_node_infos_with_relays(&nodes)
        .await
    {
        Ok(infos) => Some(infos),
        Err(e) => {
            tracing::error!(err = %e, "app: не удалось прочитать инбаунды узлов для /servers");
            None
        }
    };

    let servers: Vec<AppServer> = nodes
        .iter()
        .enumerate()
        .map(|(idx, n)| {
            let cpu = n.last_cpu.unwrap_or(0.0);
            let ram = n.last_ram.unwrap_or(0.0);
            let load = (cpu + ram) / 2.0;
            let connections = n.active_connections.unwrap_or(0);
            let is_full = n.max_users > 0 && connections >= n.max_users;
            let status = server_status(is_full, cpu).to_string();

            // `get_node_infos_with_relays` сохраняет порядок входного среза,
            // поэтому индекс — законный ключ соответствия Node ↔ NodeInfo.
            let info = node_infos.as_ref().and_then(|infos| infos.get(idx));

            let via_relay = info.and_then(|ni| ni.relay_info.as_ref()).map(|r| {
                AppRelayHop {
                    // relay_id заведомо Some: relay_info заполняется только по нему.
                    node_id: n.relay_id.unwrap_or(0),
                    name: r.name.clone(),
                    flag: country_flag(r.country_code.as_deref().unwrap_or("")),
                    country_code: r.country_code.clone(),
                    chained_in_config: false,
                }
            });

            let inbounds = info.map(|ni| build_inbound_rows(ni));

            AppServer {
                id: n.id,
                name: format!("Node #{} ({} Mbps)", n.id, n.current_speed_mbps),
                flag: country_flag(n.country_code.as_deref().unwrap_or("US")),
                country_code: n.country_code.clone(),
                latency_ms: n.last_latency.map(|l| l as i32),
                load_pct: load,
                status,
                via_relay,
                inbounds,
                inbounds_error: if node_infos.is_none() {
                    Some("panel_could_not_read_inbounds".to_string())
                } else {
                    None
                },
            }
        })
        .collect();

    match access_header {
        Some(state_name) => (
            [(crate::subscription::access::HDR_STATE, state_name)],
            Json(servers),
        )
            .into_response(),
        None => Json(servers).into_response(),
    }
}

/// Разворачивает узел в строки пикера протокола, повторяя разбор и подписи
/// генератора Clash функция в функцию: `parse_stream_settings` →
/// `format_proto_label` → `format_node_label`. Своего парсера
/// `stream_settings` здесь нет и быть не должно — второй парсер это
/// гарантированное расхождение имени с телом.
fn build_inbound_rows(ni: &crate::singbox::subscription_generator::NodeInfo) -> Vec<AppInbound> {
    use crate::singbox::subscription_generator::{
        format_node_label, format_proto_label, parse_stream_settings,
    };

    let node_label = format_node_label(ni);
    // Тот же признак, по которому генератор дописывает суффикс: наличие
    // relay_info, а не колонка relay_id (релэй мог не найтись).
    let relay_suffix = if ni.relay_info.is_some() { " ↪" } else { "" };

    let mut rows: Vec<AppInbound> = ni
        .inbounds
        .iter()
        .filter(|inbound| inbound.enable)
        .map(|inbound| {
            let si = parse_stream_settings(&inbound.stream_settings, ni);
            let label = format_proto_label(&inbound.protocol, &si);
            let reason = clash_inbound_availability(&inbound.protocol, &si.network);
            AppInbound {
                id: Some(inbound.id),
                tag: inbound.tag.clone(),
                protocol: inbound.protocol.to_ascii_lowercase(),
                network: si.network.clone(),
                security: si.security.clone(),
                port: inbound.listen_port,
                proxy_name: reason
                    .is_none()
                    .then(|| format!("{} {}{}", node_label, label, relay_suffix)),
                label,
                available: reason.is_none(),
                unavailable_reason: reason,
            }
        })
        .collect();

    // Легаси-ветка генератора: когда включённых инбаундов нет вовсе, он всё
    // равно выпускает один Reality-прокси из колонок узла. Без этой строки
    // пикер показал бы «протоколов нет» на узле, у которого в теле конфига
    // прокси есть.
    if ni.inbounds.iter().all(|i| !i.enable) && ni.reality_port.is_some() {
        rows.push(AppInbound {
            id: None,
            tag: "legacy-reality".to_string(),
            protocol: "vless".to_string(),
            network: "tcp".to_string(),
            security: "reality".to_string(),
            port: ni.reality_port.unwrap_or(0) as i64,
            label: "Reality".to_string(),
            proxy_name: Some(format!("{} Reality", node_label)),
            available: true,
            unavailable_reason: None,
        });
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::{LISTING_PLAN_SQL, build_inbound_rows, clash_inbound_availability, server_status};

    /// Вопрос «какие у меня есть сервера» не зависит от квоты, и ответ на него
    /// зависеть не должен. Пользователь, задушенный за суточный трафик, получал
    /// пустой экран серверов ровно в момент, когда ему надо было понять, что он
    /// теряет и за что платит.
    ///
    /// Тест держит границу с двух сторон: строка обязана пускать задушенную
    /// подписку И обязана предпочитать активную, если у человека есть обе, —
    /// иначе список серверов приехал бы от чужого плана.
    #[test]
    fn the_servers_listing_survives_a_quota_block() {
        assert!(
            LISTING_PLAN_SQL.contains("status IN ('active', 'throttled')"),
            "список серверов снова спрашивает только активную подписку: {}",
            LISTING_PLAN_SQL
        );
        assert!(
            LISTING_PLAN_SQL.contains("ORDER BY (status = 'active') DESC"),
            "активная подписка обязана выигрывать у задушенной: {}",
            LISTING_PLAN_SQL
        );
    }

    use crate::singbox::subscription_generator::NodeInfo;
    use caramba_db::models::network::Inbound;

    /// Инбаунд с теми полями, которые читает разбор; остальные колонки к
    /// имени прокси отношения не имеют.
    fn inbound(id: i64, tag: &str, protocol: &str, port: i64, stream_settings: &str) -> Inbound {
        Inbound {
            id,
            node_id: 1,
            tag: tag.to_string(),
            protocol: protocol.to_string(),
            listen_port: port,
            listen_ip: "0.0.0.0".to_string(),
            settings: "{}".to_string(),
            stream_settings: stream_settings.to_string(),
            remark: None,
            enable: true,
            renew_interval_mins: 0,
            port_range_start: 0,
            port_range_end: 0,
            last_rotated_at: None,
            created_at: None,
        }
    }

    fn node_info(country: &str, inbounds: Vec<Inbound>) -> NodeInfo {
        NodeInfo {
            name: "Germany".to_string(),
            address: "85.215.196.151".to_string(),
            reality_port: Some(443),
            reality_sni: Some("essentialhome.live".to_string()),
            reality_public_key: Some("3Tsh7haY915qWht_DsC4Vxunj15EBbTUo0VIIjycSDQ".to_string()),
            reality_short_id: Some("0b4bf3f48a32ccb8".to_string()),
            hy2_port: None,
            hy2_sni: None,
            frontend_url: None,
            inbounds,
            relay_info: None,
            country_code: Some(country.to_string()),
            is_relay: false,
            config_block_ads: false,
            config_block_porn: false,
            config_block_torrent: false,
        }
    }

    /// Слепок узла 1 живой панели: те же восемь включённых инбаундов с теми же
    /// `stream_settings`, что лежат в проде, и та же привязка к релэю (node 2),
    /// из-за которой генератор дописывает суффикс ` ↪`.
    ///
    /// Тест держит `/servers` и тело конфига одной строкой: он сверяет
    /// `proxy_name` с именами, которые живая подписка
    /// `feb7e480-314d-4834-8304-220db70684c2` печатает в `proxies:`. Разъедутся
    /// подписи — упадёт здесь, а не в пикере пользователя.
    #[test]
    fn inbound_rows_carry_the_exact_proxy_names_of_the_clash_body() {
        let inbounds = vec![
            inbound(
                303,
                "Hysteria2-4b6c7b66",
                "hysteria2",
                11466,
                r#"{"network":"udp","security":"tls","tlsSettings":{"serverName":"dev.portal.musikverein-maihingen.de"},"hysteria2Settings":{"ports":"11466"}}"#,
            ),
            inbound(
                304,
                "NaiveProxy-16726df0",
                "naive",
                15400,
                r#"{"network":"tcp","security":"tls","tls_settings":{"server_name":"www.dekulta.de"}}"#,
            ),
            inbound(
                305,
                "TUIC-47a0b813",
                "tuic",
                16400,
                r#"{"network":"udp","security":"tls","tlsSettings":{"serverName":"dev.portal.musikverein-maihingen.de"},"tuicSettings":{"congestion":"bbr"}}"#,
            ),
            inbound(
                306,
                "VLESS-gRPC-TLS-524d54b7",
                "vless",
                10400,
                r#"{"network":"grpc","security":"tls","tls_settings":{"server_name":"www.dekulta.de"}}"#,
            ),
            inbound(
                307,
                "VLESS-HTTPUpgrade-TLS-4cfee905",
                "vless",
                13400,
                r#"{"network":"httpupgrade","security":"tls","tls_settings":{"server_name":"www.dekulta.de"},"http_upgrade_settings":{"path":"/hu","host":"www.dekulta.de"}}"#,
            ),
            inbound(
                308,
                "VLESS-Reality-9ac11700",
                "vless",
                443,
                r#"{"network":"tcp","security":"reality","reality_settings":{"show":false,"dest":"www.dekulta.de:443","xver":0,"server_names":["www.dekulta.de"],"public_key":"3Tsh7haY915qWht_DsC4Vxunj15EBbTUo0VIIjycSDQ","short_ids":["0b4bf3f48a32ccb8"]}}"#,
            ),
            inbound(
                309,
                "VLESS-TCP-TLS-9f8eb99a",
                "vless",
                14400,
                r#"{"network":"tcp","security":"tls","tls_settings":{"server_name":"dev.portal.musikverein-maihingen.de"}}"#,
            ),
            inbound(
                310,
                "VLESS-WS-TLS-28a0a268",
                "vless",
                12400,
                r#"{"network":"ws","security":"tls","tls_settings":{"server_name":"www.dekulta.de"},"ws_settings":{"path":"/ws","headers":{"Host":"www.dekulta.de"}}}"#,
            ),
        ];

        let mut ni = node_info("DE", inbounds);
        // nodes.relay_id = 2 на живом узле 1: относится к суффиксу имени, и
        // только к нему — цепочки Clash всё равно не строит.
        ni.relay_info = Some(Box::new(node_info("RU", vec![])));

        let rows = build_inbound_rows(&ni);
        let got: Vec<(&str, &str, &str, Option<&str>, Option<&str>)> = rows
            .iter()
            .map(|r| {
                (
                    r.protocol.as_str(),
                    r.network.as_str(),
                    r.security.as_str(),
                    r.proxy_name.as_deref(),
                    r.unavailable_reason,
                )
            })
            .collect();

        assert_eq!(
            got,
            vec![
                ("hysteria2", "udp", "tls", Some("🇩🇪 Speed ↪"), None),
                // Единственная строка, под которой прокси нет — и она видна.
                (
                    "naive",
                    "tcp",
                    "tls",
                    None,
                    Some("protocol_not_emitted_by_clash")
                ),
                ("tuic", "udp", "tls", Some("🇩🇪 TUIC ↪"), None),
                ("vless", "grpc", "tls", Some("🇩🇪 Stream ↪"), None),
                ("vless", "httpupgrade", "tls", Some("🇩🇪 HTTP ↪"), None),
                ("vless", "tcp", "reality", Some("🇩🇪 Stealth ↪"), None),
                ("vless", "tcp", "tls", Some("🇩🇪 Secure ↪"), None),
                ("vless", "ws", "tls", Some("🇩🇪 WebSocket ↪"), None),
            ]
        );
    }

    /// `vless/tcp/reality` и `vless/tcp/tls` обязаны остаться РАЗНЫМИ строками
    /// пикера. Схлопни их в слово «vless» — и выбор перестанет что-либо
    /// выбирать: у них разные порты, разное имя прокси и разная маскировка.
    #[test]
    fn reality_and_plain_tls_do_not_collapse_into_one_protocol_row() {
        let ni = node_info(
            "DE",
            vec![
                inbound(
                    308,
                    "VLESS-Reality",
                    "vless",
                    443,
                    r#"{"network":"tcp","security":"reality","reality_settings":{"public_key":"k","short_ids":["s"]}}"#,
                ),
                inbound(
                    309,
                    "VLESS-TCP-TLS",
                    "vless",
                    14400,
                    r#"{"network":"tcp","security":"tls","tls_settings":{"server_name":"example.org"}}"#,
                ),
            ],
        );

        let rows = build_inbound_rows(&ni);

        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].security.as_str(), rows[0].port), ("reality", 443));
        assert_eq!((rows[1].security.as_str(), rows[1].port), ("tls", 14400));
        assert_ne!(rows[0].proxy_name, rows[1].proxy_name);
        assert_eq!(rows[0].proxy_name.as_deref(), Some("🇩🇪 Stealth"));
        assert_eq!(rows[1].proxy_name.as_deref(), Some("🇩🇪 Secure"));
    }

    /// Узел без включённых инбаундов: генератор всё равно выпускает один
    /// легаси-Reality-прокси из колонок узла, и пикер обязан показать ту же
    /// строку, а не «протоколов нет».
    #[test]
    fn a_node_without_inbounds_still_reports_the_legacy_reality_proxy() {
        let rows = build_inbound_rows(&node_info("DE", vec![]));

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, None);
        assert_eq!(rows[0].proxy_name.as_deref(), Some("🇩🇪 Reality"));
        assert!(rows[0].available);
    }

    /// Пикер протокола не должен предлагать строку, под которую в теле конфига
    /// нет прокси. Живой пример — `naive` на узле 1: инбаунд включён, имя
    /// попадает в группу `Auto-Relay`, а прокси генератор не выпускает.
    /// Здесь такая строка обязана прийти недоступной С ПРИЧИНОЙ, а не пропасть.
    #[test]
    fn a_protocol_the_clash_generator_cannot_emit_is_unavailable_not_hidden() {
        assert_eq!(
            clash_inbound_availability("naive", "tcp"),
            Some("protocol_not_emitted_by_clash")
        );
        assert_eq!(
            clash_inbound_availability("shadowtls", "tcp"),
            Some("protocol_not_emitted_by_clash")
        );
    }

    /// xhttp/splithttp — транспорт Xray, которого Clash Meta не знает.
    /// Проверяется именно `network`, а не `protocol`: у такого инбаунда
    /// протокол всё равно `vless`, и фильтр по протоколу его бы пропустил.
    #[test]
    fn the_xray_only_transport_is_judged_by_network_not_protocol() {
        assert_eq!(
            clash_inbound_availability("vless", "xhttp"),
            Some("transport_not_supported_by_clash")
        );
        assert_eq!(
            clash_inbound_availability("vless", "splithttp"),
            Some("transport_not_supported_by_clash")
        );
        assert_eq!(clash_inbound_availability("vless", "tcp"), None);
    }

    /// Полный набор веток `match inbound.protocol` в `generate_clash_config`.
    /// Тест держит зеркало зеркалом: ветку добавят — он покажет, что здесь её
    /// ещё нет (протокол числится недоступным, хотя прокси уже выпускается).
    #[test]
    fn every_protocol_the_generator_has_an_arm_for_is_available() {
        for protocol in [
            "vless",
            "vmess",
            "trojan",
            "shadowsocks",
            "ss",
            "hysteria2",
            "hy2",
            "tuic",
        ] {
            assert_eq!(
                clash_inbound_availability(protocol, "tcp"),
                None,
                "{protocol} выпускается генератором, строка обязана быть выбираемой"
            );
        }
    }

    /// Регистр протокола приходит из колонки БД и не является частью решения:
    /// `VLESS` и `vless` — один и тот же инбаунд.
    #[test]
    fn the_protocol_column_is_matched_case_insensitively() {
        assert_eq!(clash_inbound_availability("VLESS", "tcp"), None);
        assert_eq!(clash_inbound_availability("Hysteria2", "udp"), None);
    }

    /// Словарь фиксирован: приложение знает только online/busy/full. Ни одно
    /// значение колонки `nodes.status` («active») наружу выйти не должно —
    /// именно оно клало весь список выходов.
    #[test]
    fn the_wire_vocabulary_is_online_busy_full() {
        assert_eq!(server_status(false, 0.0), "online");
        assert_eq!(server_status(false, 80.0), "online");
        assert_eq!(server_status(false, 80.1), "busy");
        assert_eq!(server_status(false, 94.9), "busy");
        // Вместимость важнее загрузки: полный узел нельзя выбрать вообще, а
        // busy — можно.
        assert_eq!(server_status(true, 0.0), "full");
        assert_eq!(server_status(true, 99.0), "full");

        for status in [
            server_status(false, 0.0),
            server_status(false, 90.0),
            server_status(true, 0.0),
        ] {
            assert!(
                matches!(status, "online" | "busy" | "full"),
                "статус {status:?} вне словаря приложения"
            );
            assert_ne!(status, "active", "колонка БД не должна утекать на провод");
        }
    }
}

use crate::services::activity_service::ActivityService;
use crate::singbox::connection_variants::{
    SingboxConnectionVariant, apply_connection_variant, fixed_connection_variants,
};
use crate::singbox::subscription_generator::{NodeInfo, UserKeys};
use anyhow::{Context, Result};
use caramba_db::models::network::InboundType;
use caramba_db::models::node::Node;
use caramba_db::models::store::{
    AlertType, GiftCode, Plan, PlanDuration, RenewalResult, Subscription, SubscriptionIpTracking,
    SubscriptionWithDetails,
};
use caramba_db::repositories::node_repo::NodeRepository;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::HashSet;
use std::net::IpAddr;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SubscriptionService {
    pool: PgPool,
    // Add orchestration service for trigger-based sync
    pub orchestration_service:
        Option<std::sync::Arc<crate::services::orchestration_service::OrchestrationService>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ExpiredQuotaSubscription {
    pub subscription_id: i64,
    pub user_id: i64,
    pub node_id: Option<i64>,
    /// Plan of the subscription — quota enforcement fans node notifications
    /// out by plan, because `node_id` can be NULL or cover only one of the
    /// nodes serving the user.
    pub plan_id: i64,
}

/// Окно свежести привязки устройства — ЕДИНСТВЕННОЕ на всю панель.
///
/// Раньше их было два и они противоречили друг другу: гейт подключения считал
/// устройства за 15 минут, а уборщик удалял лизы старше часа. Устройство,
/// которым не пользовались полдня, исчезало из списка в кабинете, а вернувшись,
/// заводилось заново и съедало слот лимита. По постановке владельца привязка
/// живёт до ручной отвязки, поэтому окно длинное: месяц молчания это уже
/// «устройством не пользуются», а не «человек отошёл от компьютера».
pub const DEVICE_LEASE_TTL_DAYS: i64 = 30;

/// Как устройство представилось панели.
///
/// Наше приложение присылает эти заголовки и на запрос подписки, и на
/// `/api/v2/app/*`; сторонние клиенты (Hiddify, Clash, v2rayNG) прислать их не
/// могут, у них всё поле пустое и устройство опознаётся по User-Agent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceIdentity {
    /// Стабильный идентификатор установки приложения (`X-Caramba-Device-Id`).
    /// Переживает и обновление приложения (меняется User-Agent), и переезд
    /// между Wi-Fi и сотовой сетью (меняется адрес).
    pub client_device_id: Option<String>,
    /// Имя устройства по умолчанию (`X-Caramba-Device-Name`): модель телефона
    /// или hostname. Это именно значение по умолчанию: если человек уже
    /// переименовал устройство в кабинете, его имя не перетирается.
    pub display_name: Option<String>,
    /// Платформа (`X-Caramba-Device-Platform`): android/ios/macos/windows/linux.
    pub platform: Option<String>,
}

impl DeviceIdentity {
    pub const HEADER_ID: &'static str = "x-caramba-device-id";
    pub const HEADER_NAME: &'static str = "x-caramba-device-name";
    pub const HEADER_PLATFORM: &'static str = "x-caramba-device-platform";

    /// Заголовки приходят из интернета, поэтому чистятся и обрезаются: они
    /// попадают в имя устройства в кабинете и в ключ, по которому считается
    /// лимит.
    fn sanitize(value: Option<&str>, max_chars: usize) -> Option<String> {
        let cleaned: String = value?
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(max_chars)
            .collect();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        }
    }

    pub fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        Self {
            client_device_id: Self::sanitize(get(Self::HEADER_ID), 64),
            display_name: Self::sanitize(get(Self::HEADER_NAME), 64),
            platform: Self::sanitize(get(Self::HEADER_PLATFORM), 32)
                .map(|p| p.to_ascii_lowercase()),
        }
    }

    /// Ничего о себе не сообщили: обычный сторонний клиент.
    pub fn is_anonymous(&self) -> bool {
        self.client_device_id.is_none()
    }
}

/// User-Agent, которым представляется НАШЕ ядро, когда качает конфиг подписки
/// (`libs/caramba-core/subscription`, `ClashUserAgent`).
///
/// Нужен ровно для слияния лиз: анонимный запрос сливается с уже опознанной
/// лизой только если он пришёл от нашего же ядра. Без этой оговорки чужой
/// клиент (Hiddify, Clash) за тем же NAT прицепился бы к лизе телефона и не
/// занял бы слот лимита — то есть починка одного просчёта открыла бы другой.
pub const CORE_SUBSCRIPTION_USER_AGENT: &str = "caramba-core/1.0 (mihomo) clash.meta";

/// Окно, в котором два представления одного устройства считаются одним
/// устройством.
///
/// Десять минут — это «то же самое приложение прямо сейчас»: приложение
/// дёргает `/api/v2/app/*` и поднимает туннель в пределах одной сессии. Шире
/// окно — и за NAT начнут склеиваться разные аппараты; уже — и ядро успеет
/// завести вторую лизу, пока пользователь листает экран.
pub const DEVICE_MERGE_WINDOW_MINUTES: i64 = 10;

/// Лиза в том виде, в каком её видит РЕШЕНИЕ о слиянии.
///
/// Отдельный тип, потому что решение обязано проверяться без базы: за ним
/// стоит правило «когда два представления это одно устройство», а не запрос.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseCandidate {
    pub id: i64,
    pub client_device_id: Option<String>,
    pub user_agent: Option<String>,
    pub last_ip: String,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
}

/// Найденная лиза: её номер и идентификатор установки, который в ней УЖЕ
/// записан.
///
/// Идентификатор возвращается наружу не для красоты: по нему считается
/// отпечаток строки. Слить анонимный запрос в опознанную лизу и пересчитать её
/// отпечаток от User-Agent значило бы столкнуть её с уникальным индексом
/// `(subscription_id, device_fingerprint)` ровно на той строке, ради слияния с
/// которой всё и затевалось.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedLease {
    pub id: i64,
    pub client_device_id: Option<String>,
}

/// Два представления одного устройства: приложение называет себя
/// `client_device_id`, а ядро, качающее подписку, — одним User-Agent.
///
/// ЗАЧЕМ. Приложение шлёт `X-Caramba-Device-*` на `/api/v2/app/*`, и панель
/// заводит по ним лизу. Конфиг подписки (`/sub/{uuid}`) выкачивает Go-ядро, и
/// сборки до этой правки не присылали ничего, кроме User-Agent. Один телефон
/// заводил ДВЕ лизы и съедал два слота лимита устройств: ветка по
/// `client_device_id` и ветка по User-Agent никогда не сходились.
///
/// ЧЕМ СКЛЕИВАЕМ. Адресом и временем: две записи с одного адреса в пределах
/// [`DEVICE_MERGE_WINDOW_MINUTES`] — это один аппарат. Адрес `0.0.0.0` —
/// заглушка внутренних вызовов, по ней не склеивается ничего.
pub fn merge_lease_candidate(
    device: &DeviceIdentity,
    user_agent: Option<&str>,
    ip: &str,
    now: chrono::DateTime<chrono::Utc>,
    candidates: &[LeaseCandidate],
) -> Option<MatchedLease> {
    if ip.is_empty() || ip == "0.0.0.0" {
        return None;
    }
    let cutoff = now - Duration::minutes(DEVICE_MERGE_WINDOW_MINUTES);
    let fresh = |row: &&LeaseCandidate| row.last_ip == ip && row.last_seen_at > cutoff;

    match device.client_device_id.as_deref() {
        // Запрос назвал себя, но своей лизы ещё нет. Если рядом лежит
        // безымянная лиза того же клиента с того же адреса — это она и есть:
        // присваиваем ей идентификатор вместо того, чтобы заводить вторую.
        Some(client_device_id) => {
            let same_ua = |row: &&LeaseCandidate| {
                row.client_device_id.is_none()
                    && row.user_agent.as_deref().map(str::trim).unwrap_or("")
                        == user_agent.map(str::trim).unwrap_or("")
            };
            candidates
                .iter()
                .filter(fresh)
                .filter(same_ua)
                .max_by_key(|row| row.last_seen_at)
                .map(|row| MatchedLease {
                    id: row.id,
                    client_device_id: Some(client_device_id.to_string()),
                })
        }
        // Запрос себя не назвал. Сливаем ТОЛЬКО запрос нашего ядра: у чужого
        // клиента за тем же NAT нет никакого отношения к этому телефону.
        None => {
            let ua = user_agent.map(str::trim).unwrap_or("");
            if !ua.is_empty() && ua != CORE_SUBSCRIPTION_USER_AGENT {
                return None;
            }
            candidates
                .iter()
                .filter(fresh)
                .filter(|row| row.client_device_id.is_some())
                .max_by_key(|row| row.last_seen_at)
                .map(|row| MatchedLease {
                    id: row.id,
                    client_device_id: row.client_device_id.clone(),
                })
        }
    }
}

/// Решение гейта устройств по ОДНОМУ ключу: и «знаем ли мы это устройство», и
/// «сколько их у аккаунта» считаются одинаково, по владельцу и отпечатку.
/// Раньше внешний гейт сравнивал адреса, а внутренний отпечатки: несколько
/// телефонов за одним NAT проходили лимит насквозь, а один телефон при смене
/// сети в него упирался.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceAdmission {
    /// Устройство уже привязано к аккаунту.
    pub known: bool,
    /// Сколько устройств привязано сейчас.
    pub used: i64,
    /// Лимит плана; 0 значит безлимит.
    pub limit: i32,
}

impl DeviceAdmission {
    pub fn allowed(&self) -> bool {
        self.known || self.limit <= 0 || self.used < self.limit as i64
    }
}

/// Одна привязка устройства так, как её видит кабинет.
///
/// Один тип и один запрос на оба кабинета (мини-апп через `api/client.rs` и
/// приложение через `api/v2/app_account.rs`): раньше каждый строил свой SQL со
/// своим набором колонок и своим сроком свежести, и списки устройств у одного
/// человека в двух кабинетах не совпадали.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserDeviceLease {
    pub id: i64,
    pub subscription_id: i64,
    /// Имя, заданное человеком.
    pub display_name: Option<String>,
    /// Авто-имя из User-Agent — запасной вариант.
    pub device_name: Option<String>,
    pub platform: Option<String>,
    pub client_device_id: Option<String>,
    pub user_agent: Option<String>,
    pub last_ip: String,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
    /// Устройство выходило на связь в последние 15 минут.
    pub online: bool,
}

impl UserDeviceLease {
    /// Что показать человеку: его имя, иначе авто-имя, иначе честная заглушка.
    pub fn label(&self) -> String {
        self.display_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .or_else(|| {
                self.device_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
            })
            .unwrap_or("Unknown Device")
            .to_string()
    }
}

impl SubscriptionService {
    const INBOUND_SELECT_SQL: &'static str = r#"
        SELECT
            id,
            node_id,
            tag,
            protocol,
            listen_port::BIGINT AS listen_port,
            COALESCE(listen_ip, '::') AS listen_ip,
            COALESCE(settings, '{}') AS settings,
            COALESCE(stream_settings, '{}') AS stream_settings,
            remark,
            COALESCE(enable, TRUE) AS enable,
            COALESCE(renew_interval_mins, 0)::BIGINT AS renew_interval_mins,
            COALESCE(port_range_start, 10000)::BIGINT AS port_range_start,
            COALESCE(port_range_end, 60000)::BIGINT AS port_range_end,
            last_rotated_at,
            created_at
        FROM inbounds
    "#;

    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            orchestration_service: None,
        }
    }

    // Allow injecting orchestration service after circular dep resolution
    pub fn set_orchestration_service(
        &mut self,
        svc: std::sync::Arc<crate::services::orchestration_service::OrchestrationService>,
    ) {
        self.orchestration_service = Some(svc);
    }

    fn is_placeholder_sni(sni: &str) -> bool {
        let sni = sni.trim().to_ascii_lowercase();
        sni.is_empty()
            || sni == "www.google.com"
            || sni == "google.com"
            || sni == "drive.google.com"
    }

    fn parse_ip_maybe(value: &str) -> Option<IpAddr> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }

        if let Ok(ip) = value.parse::<IpAddr>() {
            return Some(Self::canonicalize_ip(ip));
        }
        if let Ok(sock) = value.parse::<std::net::SocketAddr>() {
            return Some(Self::canonicalize_ip(sock.ip()));
        }
        if let Some((host, _port)) = value.rsplit_once(':')
            && let Ok(ip) = host.parse::<IpAddr>()
        {
            return Some(Self::canonicalize_ip(ip));
        }
        None
    }

    fn canonicalize_ip(ip: IpAddr) -> IpAddr {
        match ip {
            IpAddr::V6(v6) => v6.to_ipv4().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
            other => other,
        }
    }

    fn normalize_client_ip(value: &str) -> Option<String> {
        let ip = Self::parse_ip_maybe(value)?;
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
            return None;
        }
        Some(ip.to_string())
    }

    /// Не чаще одного предупреждения в 10 минут на подписку: клиенты опрашивают
    /// подписку постоянно, и без ограничения журнал забьётся одной и той же строкой.
    fn should_warn_unattributed_ip(sub_id: i64) -> bool {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        use std::time::{Duration, Instant};

        static LAST: OnceLock<Mutex<HashMap<i64, Instant>>> = OnceLock::new();
        let map = LAST.get_or_init(|| Mutex::new(HashMap::new()));
        let now = Instant::now();
        let Ok(mut guard) = map.lock() else {
            // Отравленный мьютекс: лучше лишняя строка в журнале, чем паника.
            return true;
        };
        if guard.len() > 10_000 {
            guard.retain(|_, t| now.duration_since(*t) < Duration::from_secs(3600));
        }
        match guard.get(&sub_id) {
            Some(t) if now.duration_since(*t) < Duration::from_secs(600) => false,
            _ => {
                guard.insert(sub_id, now);
                true
            }
        }
    }

    async fn infrastructure_ips(&self) -> HashSet<IpAddr> {
        let mut rows: Vec<String> = sqlx::query_scalar("SELECT ip FROM nodes")
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        let mut frontend_rows: Vec<String> =
            sqlx::query_scalar("SELECT ip_address FROM frontend_servers")
                .fetch_all(&self.pool)
                .await
                .unwrap_or_default();
        rows.append(&mut frontend_rows);
        rows.into_iter()
            .filter_map(|ip| Self::parse_ip_maybe(&ip))
            .collect()
    }

    fn should_track_device_ip(raw: &str, infra_ips: &HashSet<IpAddr>) -> bool {
        let Some(ip) = Self::parse_ip_maybe(raw) else {
            return false;
        };
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
            return false;
        }
        !infra_ips.contains(&ip)
    }

    fn normalize_user_agent(user_agent: Option<&str>) -> Option<String> {
        user_agent
            .map(str::trim)
            .filter(|ua| !ua.is_empty())
            .map(|ua| ua.to_string())
    }

    /// Отпечаток устройства — ключ, по которому панель узнаёт «это то же самое
    /// устройство».
    ///
    /// Раньше в него подмешивался `subscription_id`, и это ломало саму
    /// постановку: устройство привязывается к аккаунту, а строка подписки
    /// меняется при каждой смене тарифа. После смены тарифа все телефоны
    /// человека считались новыми и упирались в лимит на его же устройствах.
    ///
    /// Приложение со своим `client_device_id` опознаётся по нему одному: это
    /// единственное, что переживает и обновление приложения (новый
    /// User-Agent), и переезд между сетями (новый адрес). Сторонним клиентам
    /// остаётся User-Agent, но уже в паре с владельцем.
    ///
    /// Адрес в отпечаток не входит намеренно: у мобильного клиента он меняется
    /// по нескольку раз в день.
    pub fn device_fingerprint_for(
        user_id: i64,
        client_device_id: Option<&str>,
        user_agent: Option<&str>,
    ) -> String {
        let material = match client_device_id {
            Some(device_id) => format!("dev:{}", device_id),
            None => format!("user:{}|ua:{}", user_id, user_agent.unwrap_or("unknown")),
        };
        let mut hasher = Sha256::new();
        hasher.update(material.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn subscription_user_uuid(sub: &Subscription) -> Option<String> {
        sub.vless_uuid
            .as_deref()
            .map(str::trim)
            .filter(|uuid| !uuid.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                let fallback = sub.subscription_uuid.trim();
                if fallback.is_empty() {
                    None
                } else {
                    Some(fallback.to_string())
                }
            })
    }

    /// Владелец подписки. Лизы считаются по нему: аккаунт переживает смену
    /// тарифа, строка подписки — нет.
    async fn lease_user_id(&self, subscription_id: i64) -> Result<i64> {
        sqlx::query_scalar("SELECT user_id FROM subscriptions WHERE id = $1")
            .bind(subscription_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to resolve the subscription owner for a device lease")
    }

    /// Ищет уже известную лизу этого устройства среди ВСЕХ лиз аккаунта.
    async fn find_device_lease(
        &self,
        user_id: i64,
        device: &DeviceIdentity,
        fingerprint: &str,
        user_agent: Option<&str>,
        ip: &str,
    ) -> Result<Option<MatchedLease>> {
        // Приложение узнаётся по своему идентификатору в первую очередь: его
        // User-Agent меняется при каждом обновлении, а идентификатор нет.
        if let Some(client_device_id) = device.client_device_id.as_deref() {
            let found: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM subscription_device_leases \
                 WHERE user_id = $1 AND client_device_id = $2 \
                 ORDER BY last_seen_at DESC LIMIT 1",
            )
            .bind(user_id)
            .bind(client_device_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to look up a device lease by client device id")?;

            if let Some(id) = found {
                return Ok(Some(MatchedLease {
                    id,
                    client_device_id: Some(client_device_id.to_string()),
                }));
            }
        }

        // Слияние двух представлений одного устройства. Спрашивается РАНЬШЕ
        // ветки по User-Agent: анонимный запрос нашего же ядра обязан попасть в
        // лизу, которую приложение уже завело по идентификатору, а не завести
        // рядом вторую — ровно она и съедала второй слот лимита.
        if let Some(merged) = self
            .merge_device_lease(user_id, device, user_agent, ip)
            .await?
        {
            return Ok(Some(merged));
        }

        // Второе условие подбирает лизы, заведённые ДО перехода на отпечаток от
        // владельца: в их device_fingerprint зашит старый subscription_id, и по
        // отпечатку их уже не найти, зато User-Agent тот же. Без этого каждое
        // устройство один раз завелось бы заново и съело лишний слот лимита.
        //
        // Строки с непустым client_device_id из этой ветки исключены: там
        // устройство уже опознано выше, и чужое приложение с тем же
        // User-Agent не имеет права прицепиться к его лизе.
        let legacy_ua = user_agent.unwrap_or("unknown");
        let found: Option<(i64, Option<String>)> = sqlx::query_as(
            "SELECT id, client_device_id FROM subscription_device_leases \
             WHERE user_id = $1 \
               AND (device_fingerprint = $2 \
                    OR (client_device_id IS NULL \
                        AND COALESCE(NULLIF(user_agent, \'\'), \'unknown\') = $3)) \
             ORDER BY last_seen_at DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(fingerprint)
        .bind(legacy_ua)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to look up a device lease by fingerprint")?;

        Ok(found.map(|(id, lease_device_id)| MatchedLease {
            id,
            // Идентификатор запроса сильнее записанного: ветка подобрала
            // безымянную строку, и этот вызов её именует.
            client_device_id: device.client_device_id.clone().or(lease_device_id),
        }))
    }

    /// Свежие лизы аккаунта с ЭТОГО адреса — вход решения о слиянии.
    ///
    /// Запрос узкий намеренно: решение принимает чистая функция
    /// [`merge_lease_candidate`], а база отдаёт ей только те строки, среди
    /// которых слияние вообще возможно.
    async fn merge_device_lease(
        &self,
        user_id: i64,
        device: &DeviceIdentity,
        user_agent: Option<&str>,
        ip: &str,
    ) -> Result<Option<MatchedLease>> {
        // Заглушка внутренних вызовов: по ней не склеивается ничего, и запрос
        // в базу ради заведомого «нет» не делается.
        if ip.is_empty() || ip == "0.0.0.0" {
            return Ok(None);
        }
        let now = Utc::now();
        let cutoff = now - Duration::minutes(DEVICE_MERGE_WINDOW_MINUTES);
        let rows: Vec<LeaseCandidate> = sqlx::query_as(
            "SELECT id, client_device_id, user_agent, last_ip, last_seen_at \
             FROM subscription_device_leases \
             WHERE user_id = $1 AND last_ip = $2 AND last_seen_at > $3 \
             ORDER BY last_seen_at DESC LIMIT 20",
        )
        .bind(user_id)
        .bind(ip)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .context("Failed to load device leases for a merge decision")?
        .into_iter()
        .map(
            |(id, client_device_id, user_agent, last_ip, last_seen_at)| LeaseCandidate {
                id,
                client_device_id,
                user_agent,
                last_ip,
                last_seen_at,
            },
        )
        .collect();

        Ok(merge_lease_candidate(device, user_agent, ip, now, &rows))
    }

    /// Сколько устройств привязано к аккаунту в окне свежести.
    async fn count_user_devices(&self, user_id: i64) -> Result<i64> {
        let cutoff = Utc::now() - Duration::days(DEVICE_LEASE_TTL_DAYS);
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM subscription_device_leases \
             WHERE user_id = $1 AND last_seen_at > $2 AND last_ip <> \'0.0.0.0\'",
        )
        .bind(user_id)
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await
        .context("Failed to count device leases for the account")
    }

    /// Гейт лимита устройств: один ответ на вопрос «пускать ли это устройство».
    ///
    /// Вызывается ДО выдачи конфига (`subscription_handler`) и повторяется
    /// внутри `upsert_device_lease` как страховка от гонки. Оба считают по
    /// одному ключу и одному окну — расхождения, из-за которого NAT пробивал
    /// лимит, а смена сети в него упиралась, больше нет.
    ///
    /// `client_ip` обязателен по той же причине: слияние двух представлений
    /// одного устройства опирается на адрес, и гейт, не знающий адреса, отказал
    /// бы телефону, лизу которого запись через мгновение нашла бы.
    pub async fn check_device_admission(
        &self,
        subscription_id: i64,
        device: &DeviceIdentity,
        user_agent: Option<&str>,
        client_ip: &str,
    ) -> Result<DeviceAdmission> {
        let user_id = self.lease_user_id(subscription_id).await?;
        let normalized_ua = Self::normalize_user_agent(user_agent);
        let normalized_ip = Self::normalize_client_ip(client_ip).unwrap_or_default();
        let fingerprint = Self::device_fingerprint_for(
            user_id,
            device.client_device_id.as_deref(),
            normalized_ua.as_deref(),
        );
        let known = self
            .find_device_lease(
                user_id,
                device,
                &fingerprint,
                normalized_ua.as_deref(),
                &normalized_ip,
            )
            .await?
            .is_some();
        let limit = self
            .get_subscription_device_limit(subscription_id)
            .await
            .unwrap_or(0);
        let used = self.count_user_devices(user_id).await.unwrap_or(0);

        Ok(DeviceAdmission { known, used, limit })
    }

    async fn upsert_device_lease(
        &self,
        subscription_id: i64,
        normalized_ip: &str,
        user_agent: Option<&str>,
        node_id: Option<i64>,
        device: &DeviceIdentity,
    ) -> Result<()> {
        let user_id = self.lease_user_id(subscription_id).await?;
        let normalized_ua = Self::normalize_user_agent(user_agent);

        // Хартбит ноды приходит без User-Agent и без заголовков приложения: по
        // нему нельзя завести устройство, можно только освежить уже известное
        // по адресу. Ищем по аккаунту, а не по подписке: лиза могла переехать
        // на другую строку при смене тарифа.
        if normalized_ua.is_none() && device.is_anonymous() {
            let touched = sqlx::query(
                "UPDATE subscription_device_leases
                 SET last_seen_at = CURRENT_TIMESTAMP,
                     last_node_id = COALESCE($3, last_node_id)
                 WHERE user_id = $1 AND last_ip = $2",
            )
            .bind(user_id)
            .bind(normalized_ip)
            .bind(node_id)
            .execute(&self.pool)
            .await
            .context("Failed to refresh existing device lease by IP")?;

            if touched.rows_affected() > 0 {
                return Ok(());
            }
        }

        let fingerprint = Self::device_fingerprint_for(
            user_id,
            device.client_device_id.as_deref(),
            normalized_ua.as_deref(),
        );

        // Авто-имя считается только когда есть из чего: иначе хартбит без UA
        // затёр бы «iPhone» на «Connection Client».
        let auto_name = normalized_ua
            .as_deref()
            .map(|ua| self.parse_device_name(ua))
            .or_else(|| {
                device
                    .client_device_id
                    .as_ref()
                    .map(|_| "Caramba Connect".to_string())
            });

        if let Some(matched) = self
            .find_device_lease(
                user_id,
                device,
                &fingerprint,
                normalized_ua.as_deref(),
                normalized_ip,
            )
            .await?
        {
            // Слияние НЕ трогает отпечаток опознанной строки (NULL в COALESCE
            // ниже оставляет колонку как есть).
            //
            // Иначе слияние ломало бы само себя: анонимный запрос ядра,
            // попавший в лизу приложения, пересчитал бы её отпечаток с
            // `dev:<id>` на `user:N|ua:...` и налетел на уникальный индекс
            // `(subscription_id, device_fingerprint)` — на той самой второй
            // строке, ради избавления от которой слияние и заведено.
            let merged_into_identified =
                device.is_anonymous() && matched.client_device_id.is_some();
            let lease_fingerprint = if merged_into_identified {
                None
            } else {
                Some(fingerprint.as_str())
            };
            // Анонимный запрос не переименовывает опознанное устройство:
            // «Pixel 8» не имеет права стать «Mihomo» оттого, что конфиг
            // подписки качает ядро.
            let auto_name = if merged_into_identified {
                None
            } else {
                auto_name
            };
            // Устройство уже известно — обновляем строку на месте. Лимит здесь
            // не проверяется: он гейт подключения нового устройства, а не повод
            // отвязать уже привязанное.
            //
            // display_name ставится только если пусто: имя от приложения это
            // значение по умолчанию, а имя, которое человек задал в кабинете,
            // перетирать нельзя.
            sqlx::query(
                "UPDATE subscription_device_leases SET
                     subscription_id = $1,
                     device_fingerprint = COALESCE($2, device_fingerprint),
                     device_name = COALESCE($3, device_name),
                     display_name = COALESCE(display_name, $4),
                     user_agent = COALESCE($5, user_agent),
                     platform = COALESCE($6, platform),
                     client_device_id = COALESCE($7, client_device_id),
                     last_ip = $8,
                     last_seen_at = CURRENT_TIMESTAMP,
                     last_node_id = COALESCE($9, last_node_id)
                 WHERE id = $10",
            )
            .bind(subscription_id)
            .bind(lease_fingerprint)
            .bind(auto_name.as_deref())
            .bind(device.display_name.as_deref())
            .bind(normalized_ua.as_deref())
            .bind(device.platform.as_deref())
            .bind(matched.client_device_id.as_deref())
            .bind(normalized_ip)
            .bind(node_id)
            .bind(matched.id)
            .execute(&self.pool)
            .await
            .context("Failed to refresh the known device lease")?;

            return Ok(());
        }

        // Новое устройство: страховка от гонки с внешним гейтом. Тот же ключ и
        // то же окно, что и в check_device_admission.
        let device_limit = self
            .get_subscription_device_limit(subscription_id)
            .await
            .unwrap_or(0);
        if device_limit > 0 {
            let used = self.count_user_devices(user_id).await.unwrap_or(0);
            if used >= device_limit as i64 {
                return Err(anyhow::anyhow!(
                    "Device limit reached ({}/{})",
                    used,
                    device_limit
                ));
            }
        }

        sqlx::query(
            r#"
            INSERT INTO subscription_device_leases
                (subscription_id, user_id, device_fingerprint, device_name, display_name,
                 user_agent, platform, client_device_id, last_ip, first_seen_at, last_seen_at,
                 last_node_id)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, $10)
            ON CONFLICT (subscription_id, device_fingerprint)
            DO UPDATE SET
                device_name = COALESCE(EXCLUDED.device_name, subscription_device_leases.device_name),
                display_name = COALESCE(subscription_device_leases.display_name, EXCLUDED.display_name),
                user_agent = COALESCE(EXCLUDED.user_agent, subscription_device_leases.user_agent),
                platform = COALESCE(EXCLUDED.platform, subscription_device_leases.platform),
                client_device_id = COALESCE(EXCLUDED.client_device_id, subscription_device_leases.client_device_id),
                last_ip = EXCLUDED.last_ip,
                last_seen_at = CURRENT_TIMESTAMP,
                last_node_id = COALESCE(EXCLUDED.last_node_id, subscription_device_leases.last_node_id)
            "#,
        )
        .bind(subscription_id)
        .bind(user_id)
        .bind(&fingerprint)
        .bind(auto_name.as_deref())
        .bind(device.display_name.as_deref())
        .bind(normalized_ua.as_deref())
        .bind(device.platform.as_deref())
        .bind(device.client_device_id.as_deref())
        .bind(normalized_ip)
        .bind(node_id)
        .execute(&self.pool)
        .await
        .context("Failed to upsert subscription device lease")?;

        Ok(())
    }

    async fn get_active_ips_legacy(
        &self,
        subscription_id: i64,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<SubscriptionIpTracking>> {
        sqlx::query_as::<_, SubscriptionIpTracking>(
            "SELECT sip.*
             FROM subscription_ip_tracking sip
             WHERE sip.subscription_id = $1
               AND sip.last_seen_at > $2
               AND sip.client_ip <> '0.0.0.0'
             ORDER BY sip.last_seen_at DESC",
        )
        .bind(subscription_id)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active IPs from legacy tracking")
    }

    pub async fn get_active_plans(&self) -> Result<Vec<Plan>> {
        let mut plans = sqlx::query_as::<_, Plan>(
            "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_active = TRUE"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active plans")?;

        if plans.is_empty() {
            return Ok(Vec::new());
        }

        let plan_ids: Vec<i64> = plans.iter().map(|p| p.id).collect();
        let query =
            "SELECT * FROM plan_durations WHERE plan_id = ANY($1) ORDER BY duration_days ASC";

        let all_durations = sqlx::query_as::<_, PlanDuration>(query)
            .bind(&plan_ids)
            .fetch_all(&self.pool)
            .await?;

        for plan in &mut plans {
            plan.durations = all_durations
                .iter()
                .filter(|d| d.plan_id == plan.id)
                .cloned()
                .collect();
        }

        Ok(plans)
    }

    pub async fn convert_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2",
        )
        .bind(sub_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .context("Subscription not found")?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!(
                "Only pending subscriptions can be converted to gifts"
            ));
        }

        let duration = sub.expires_at - sub.created_at;
        let duration_days = duration.num_days() as i32;

        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        let code = format!(
            "CARAMBA-GIFT-{}",
            Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("CODE")
                .to_uppercase()
        );

        sqlx::query(
            "INSERT INTO gift_codes (code, plan_id, duration_days, created_by_user_id) VALUES ($1, $2, $3, $4)"
        )
        .bind(&code)
        .bind(sub.plan_id)
        .bind(duration_days)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Gift Code",
            &format!("Converted sub {} to gift: {}", sub_id, code),
        )
        .await;

        tx.commit().await?;
        Ok(code)
    }

    /// NOT THE LIVE GIFT-CODE PATH. The runtime redemption used by the API and
    /// bot goes through `PromoService::redeem_code` -> its own private
    /// `redeem_gift_code`, which carries the manual_approval license gate. This
    /// method has no callers (kept for reference only); the license gate below
    /// is therefore dormant. Do not treat its presence as live coverage.
    pub async fn redeem_gift_code(&self, user_id: i64, code: &str) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let gift_code_opt = sqlx::query_as::<_, GiftCode>(
            "SELECT * FROM gift_codes
             WHERE code = $1
               AND redeemed_by_user_id IS NULL
               AND COALESCE(status, 'active') = 'active'
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)",
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let gift_code =
            gift_code_opt.ok_or_else(|| anyhow::anyhow!("Invalid or already redeemed code"))?;

        let days = gift_code
            .duration_days
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no duration)"))?;
        let plan_id = gift_code
            .plan_id
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no plan)"))?;

        let expires_at = Utc::now() + Duration::days(days as i64);
        let vless_uuid = Uuid::new_v4().to_string();
        let subscription_uuid = Uuid::new_v4().to_string();

        // License gate (P4, contract E): manual_approval (Free) -> new sub stays
        // 'pending' until an admin approves; Pro -> auto-'active'. Reuses the
        // existing pending/active lifecycle; never re-pends an existing sub.
        let limits = crate::license::effective_limits_from_pool(&self.pool).await;
        let initial_status = crate::license::initial_subscription_status(&limits);

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, expires_at, status, subscription_uuid)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, user_id, plan_id, node_id, vless_uuid, expires_at, status, used_traffic, traffic_updated_at, created_at, note, auto_renew, alerts_sent, is_trial, subscription_uuid, last_sub_access
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .bind(initial_status)
        .bind(subscription_uuid)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = $1, redeemed_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(user_id)
            .bind(gift_code.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        // Trigger Sync
        if let Some(orch) = &self.orchestration_service {
            if let Some(node_id) = sub.node_id {
                let _ = orch.notify_node_update(node_id).await;
            } else {
                // If no node specific, notify all active nodes just in case (e.g. distributed plan)
                // Or we could let the nodes pull periodically.
                // For immediate effect on distributed setups, we might want to notify all linked to plan.
                // But orchestration service handles single node.
                // We can get nodes for plan and notify them.
                // For now, let's keep it simple and notify if node_id is present.
            }
        }

        Ok(sub)
    }

    pub async fn transfer(
        &self,
        sub_id: i64,
        current_user_id: i64,
        target_user_id: i64,
    ) -> Result<Subscription> {
        let sub = sqlx::query_as::<_, Subscription>(
            "UPDATE subscriptions SET user_id = $1 WHERE id = $2 AND user_id = $3 RETURNING *",
        )
        .bind(target_user_id)
        .bind(sub_id)
        .bind(current_user_id)
        .fetch_one(&self.pool)
        .await?;

        let _ = ActivityService::log(
            &self.pool,
            "Transfer",
            &format!(
                "Transferred sub {} from {} to {}",
                sub_id, current_user_id, target_user_id
            ),
        )
        .await;

        Ok(sub)
    }

    pub async fn admin_delete(&self, sub_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete subscription")?;
        Ok(())
    }

    pub async fn admin_extend(&self, sub_id: i64, days: i32) -> Result<()> {
        // Renewal semantics: extending a subscription starts a fresh traffic
        // period, so reset used_traffic AND lift a quota-driven block
        // ('expired'/'throttled') back to 'active'. Without the status flip a
        // paying user whose subscription quota-expired would stay excluded
        // from node configs even after the renewal. Other statuses (e.g.
        // 'pending' awaiting admin approval) are left untouched.
        //
        // Base time is GREATEST(now, expires_at): extending a long-expired
        // subscription from its old expires_at could leave it 'active' with a
        // past expiry, which check_expirations would immediately flip back to
        // 'expired' (status flapping + wasted config regens).
        let plan_id: Option<i64> = sqlx::query_scalar(
            r#"
            UPDATE subscriptions
            SET expires_at = GREATEST(expires_at, CURRENT_TIMESTAMP) + ($1 * interval '1 day'),
                used_traffic = 0,
                status = CASE WHEN status IN ('expired', 'throttled') THEN 'active' ELSE status END
            WHERE id = $2
            RETURNING plan_id
            "#,
        )
        .bind(days)
        .bind(sub_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to extend subscription")?;

        let _ = ActivityService::log(
            &self.pool,
            "Admin Action",
            &format!("Admin extended sub {} by {} days", sub_id, days),
        )
        .await;

        // Notify every node serving the plan so the (possibly reactivated)
        // subscription reappears in configs — node_id alone can be NULL or
        // cover only one of the plan's nodes.
        if let (Some(orch), Some(pid)) = (&self.orchestration_service, plan_id) {
            let _ = orch.notify_nodes_for_plans(&[pid]).await;
        }

        Ok(())
    }

    /// Admin approval for a manual-approval (Free-tier) subscription: flips a
    /// 'pending' subscription to 'active' so its config gets handed out.
    ///
    /// Only acts on 'pending' rows — it WILL NOT re-pend or otherwise alter an
    /// already-'active' subscription (returns Ok with `false` when nothing was
    /// pending). Triggers a node config sync after activation so the newly
    /// approved user is provisioned immediately.
    pub async fn approve_subscription(&self, sub_id: i64) -> Result<bool> {
        let updated = sqlx::query_scalar::<_, Option<i64>>(
            "UPDATE subscriptions SET status = 'active' \
             WHERE id = $1 AND status = 'pending' RETURNING node_id",
        )
        .bind(sub_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to approve subscription")?;

        let Some(node_id_opt) = updated else {
            // Row did not exist or was not pending: no-op, never re-pend active.
            return Ok(false);
        };

        let _ = ActivityService::log(
            &self.pool,
            "Admin Action",
            &format!("Admin approved pending subscription {}", sub_id),
        )
        .await;

        if let (Some(orch), Some(nid)) = (&self.orchestration_service, node_id_opt) {
            let _ = orch.notify_node_update(nid).await;
        }

        Ok(true)
    }

    pub async fn admin_gift_subscription(
        &self,
        user_id: i64,
        plan_id: i64,
        duration_days: i32,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let node_id: i64 =
            sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active' LIMIT 1")
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| anyhow::anyhow!("No active nodes available to assign"))?;

        let vless_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days as i64);
        let sub_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, subscription_uuid, created_at)
            VALUES ($1, $2, $3, $4, $5, 'active', $6, CURRENT_TIMESTAMP)
            RETURNING *
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(node_id)
        .bind(vless_uuid)
        .bind(expires_at)
        .bind(sub_uuid)
        .fetch_one(&mut *tx)
        .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Admin Action",
            &format!("Admin gifted sub to user {}", user_id),
        )
        .await;

        tx.commit().await?;

        // Trigger Sync
        if let Some(orch) = &self.orchestration_service {
            let _ = orch.notify_node_update(node_id).await;
        }

        Ok(sub)
    }

    pub async fn get_subscriptions_with_details_for_admin(
        &self,
        user_id: i64,
    ) -> Result<Vec<caramba_db::models::store::SubscriptionWithPlan>> {
        let mut subs = sqlx::query_as::<_, caramba_db::models::store::SubscriptionWithPlan>(
            r#"
            SELECT 
                s.id, 
                p.name as plan_name, 
                COALESCE(s.expires_at, s.created_at, CURRENT_TIMESTAMP) as expires_at, 
                COALESCE(s.created_at, CURRENT_TIMESTAMP) as created_at,
                COALESCE(s.status, 'pending') as status,
                0::bigint as price, 
                0::bigint as active_devices,
                COALESCE(p.device_limit, 0)::bigint as device_limit,
                COALESCE(s.used_traffic, 0)::bigint as used_traffic,
                COALESCE(p.traffic_limit_gb, 0)::bigint as traffic_limit_gb
            FROM subscriptions s
            JOIN plans p ON s.plan_id = p.id
            WHERE s.user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user subscriptions with details")?;

        for sub in &mut subs {
            sub.active_devices = self
                .get_active_ips(sub.id)
                .await
                .map(|ips| ips.len() as i64)
                .unwrap_or(0);
        }

        Ok(subs)
    }

    pub async fn get_user_subscriptions(
        &self,
        user_id: i64,
    ) -> Result<Vec<SubscriptionWithDetails>> {
        let subs = sqlx::query_as::<_, Subscription>(
            r#"
            SELECT
                id,
                user_id,
                plan_id,
                node_id,
                vless_uuid,
                COALESCE(subscription_uuid, CONCAT('legacy-', id::text)) AS subscription_uuid,
                COALESCE(status, 'pending') AS status,
                COALESCE(used_traffic, 0)::bigint AS used_traffic,
                device_count,
                activated_at,
                COALESCE(expires_at, created_at, CURRENT_TIMESTAMP) AS expires_at,
                COALESCE(created_at, CURRENT_TIMESTAMP) AS created_at,
                traffic_updated_at,
                note,
                COALESCE(auto_renew, FALSE) AS auto_renew,
                COALESCE(alerts_sent, '[]') AS alerts_sent,
                COALESCE(is_trial, FALSE) AS is_trial,
                last_sub_access,
                last_access_ip,
                last_access_ua,
                organization_id,
                relay_country,
                last_daily_topup_at
            FROM subscriptions
            WHERE user_id = $1
            ORDER BY COALESCE(created_at, CURRENT_TIMESTAMP) DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        if subs.is_empty() {
            return Ok(Vec::new());
        }

        // Resolve plan names by id regardless of plan status: a subscription
        // bought on a since-disabled plan must still show its real name, not
        // "Unknown Plan" (which also leaks into the extend dialog subtitle).
        let plan_ids: Vec<i64> = subs.iter().map(|s| s.plan_id).collect();
        let plans: Vec<(i64, String, Option<String>, i32)> = match sqlx::query_as(
            "SELECT id, name, description, traffic_limit_gb FROM plans WHERE id = ANY($1)",
        )
        .bind(&plan_ids)
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                warn!(
                    "Failed to fetch plans while building user subscriptions for {}: {}",
                    user_id, e
                );
                Vec::new()
            }
        };
        let mut result = Vec::new();

        for sub in subs {
            let plan = plans.iter().find(|p| p.0 == sub.plan_id);
            let (name, desc, limit) = if let Some(p) = plan {
                (p.1.clone(), p.2.clone(), Some(p.3))
            } else {
                ("Unknown Plan".to_string(), None, None)
            };

            result.push(SubscriptionWithDetails {
                sub,
                plan_name: name,
                plan_description: desc,
                traffic_limit_gb: limit,
            });
        }

        Ok(result)
    }

    pub async fn update_note(&self, sub_id: i64, note: String) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET note = $1 WHERE id = $2")
            .bind(note)
            .bind(sub_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_auto_renewal(&self, subscription_id: i64) -> Result<bool> {
        let current: Option<bool> = sqlx::query_scalar::<_, Option<bool>>(
            "SELECT auto_renew FROM subscriptions WHERE id = $1",
        )
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await?;

        let new_value = !current.unwrap_or(false);
        sqlx::query("UPDATE subscriptions SET auto_renew = $1 WHERE id = $2")
            .bind(new_value)
            .bind(subscription_id)
            .execute(&self.pool)
            .await?;

        Ok(new_value)
    }

    pub async fn process_auto_renewals(&self) -> Result<Vec<RenewalResult>> {
        let subs = sqlx::query_as::<_, (i64, i64, i64, String, i64)>(
            "SELECT s.id, s.user_id, s.plan_id, p.name, u.balance 
             FROM subscriptions s
             JOIN users u ON s.user_id = u.id
             JOIN plans p ON s.plan_id = p.id
             WHERE s.auto_renew = TRUE
             AND s.status = 'active'
             AND s.expires_at BETWEEN CURRENT_TIMESTAMP AND CURRENT_TIMESTAMP + interval '1 day'",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = vec![];
        for (sub_id, user_id, plan_id, plan_name, balance) in subs {
            let (price, duration_days) = sqlx::query_as::<_, (i64, i32)>(
                "SELECT price, duration_days FROM plan_durations WHERE plan_id = $1 ORDER BY duration_days LIMIT 1"
            )
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;

            if balance >= price {
                // Charge atomically with a conditional deduction: the `balance` read
                // above can be stale if the user spent elsewhere, so re-check at charge
                // time. rows_affected()==0 means insufficient funds -> abort the renewal
                // without extending. Charge + extend share one transaction.
                let mut tx = self.pool.begin().await?;

                let charged = sqlx::query(
                    "UPDATE users SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
                )
                .bind(price)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;

                if charged.rows_affected() != 1 {
                    tx.rollback().await?;
                    results.push(RenewalResult::InsufficientFunds {
                        user_id,
                        sub_id,
                        required: price,
                        available: balance,
                    });
                    continue;
                }

                // Используем фактический duration_days из plan_durations вместо захардкоженных 30 дней
                sqlx::query("UPDATE subscriptions SET expires_at = expires_at + ($1 * interval '1 day'), used_traffic = 0 WHERE id = $2")
                    .bind(duration_days)
                    .bind(sub_id)
                    .execute(&mut *tx)
                    .await?;

                tx.commit().await?;

                // Notify node to regenerate config with renewed subscription
                let node_id: Option<i64> =
                    sqlx::query_scalar("SELECT node_id FROM subscriptions WHERE id = $1")
                        .bind(sub_id)
                        .fetch_optional(&self.pool)
                        .await
                        .ok()
                        .flatten();

                if let (Some(orch), Some(nid)) = (&self.orchestration_service, node_id) {
                    let _ = orch.notify_node_update(nid).await;
                }

                results.push(RenewalResult::Success {
                    user_id,
                    sub_id,
                    amount: price,
                    plan_name,
                });
            } else {
                results.push(RenewalResult::InsufficientFunds {
                    user_id,
                    sub_id,
                    required: price,
                    available: balance,
                });
            }
        }
        Ok(results)
    }

    pub async fn get_subscription_links(&self, sub_id: i64) -> Result<Vec<String>> {
        let mut links = Vec::new();
        let sub: Option<Subscription> = sqlx::query_as("SELECT * FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(sub) = sub {
            let Some(uuid) = Self::subscription_user_uuid(&sub) else {
                warn!(
                    "Subscription {} has neither vless_uuid nor subscription_uuid; direct links unavailable",
                    sub.id
                );
                return Ok(links);
            };
            let inbounds = sqlx::query_as::<_, caramba_db::models::network::Inbound>(
                r#"
                SELECT DISTINCT i.id,
                       i.node_id,
                       i.tag,
                       i.protocol,
                       i.listen_port::BIGINT AS listen_port,
                       COALESCE(i.listen_ip, '::') AS listen_ip,
                       COALESCE(i.settings, '{}') AS settings,
                       COALESCE(i.stream_settings, '{}') AS stream_settings,
                       i.remark,
                       COALESCE(i.enable, TRUE) AS enable,
                       COALESCE(i.renew_interval_mins, 0)::BIGINT AS renew_interval_mins,
                       COALESCE(i.port_range_start, 10000)::BIGINT AS port_range_start,
                       COALESCE(i.port_range_end, 60000)::BIGINT AS port_range_end,
                       i.last_rotated_at,
                       i.created_at
                FROM inbounds i
                LEFT JOIN plan_inbounds pi ON pi.inbound_id = i.id
                LEFT JOIN plan_nodes pn ON pn.node_id = i.node_id
                LEFT JOIN node_group_members ngm ON ngm.node_id = i.node_id
                LEFT JOIN plan_groups pg ON pg.group_id = ngm.group_id
                WHERE (pi.plan_id = $1 OR pn.plan_id = $1 OR pg.plan_id = $1) AND i.enable = TRUE
                "#,
            )
            .bind(sub.plan_id)
            .fetch_all(&self.pool)
            .await?;

            for inbound in inbounds {
                use caramba_db::models::network::StreamSettings;
                let stream: StreamSettings =
                    serde_json::from_str(&inbound.stream_settings).unwrap_or_default();
                let security = stream.security.as_deref().unwrap_or("none");
                let network = stream.network.as_deref().unwrap_or("tcp");

                let node_details: Option<(
                    String,
                    Option<String>,
                    Option<String>,
                    String,
                    Option<String>,
                )> = sqlx::query_as(
                    "SELECT ip, reality_pub, short_id, name, reality_sni FROM nodes WHERE id = $1",
                )
                .bind(inbound.node_id)
                .fetch_optional(&self.pool)
                .await?;

                let (node_ip, reality_pub, short_id, node_name, node_reality_sni) =
                    if let Some((ip, pub_key, sid, name, reality_sni)) = node_details {
                        (ip, pub_key, sid, name, reality_sni)
                    } else {
                        (
                            inbound.listen_ip.clone(),
                            None,
                            None,
                            format!("node-{}", inbound.node_id),
                            None,
                        )
                    };

                let address = if inbound.listen_ip == "::" || inbound.listen_ip == "0.0.0.0" {
                    node_ip
                } else {
                    inbound.listen_ip.clone()
                };

                let node_sni = node_reality_sni.filter(|s| !Self::is_placeholder_sni(s));
                let port = inbound.listen_port;
                let protocol_label = inbound.protocol.to_lowercase();
                let transport_label = if network.trim().is_empty() {
                    "tcp".to_string()
                } else {
                    network.to_lowercase()
                };
                let remark = format!(
                    "{}-{} {}-{}",
                    node_name, inbound.node_id, protocol_label, transport_label
                );
                let encoded_remark = urlencoding::encode(&remark).to_string();

                match inbound.protocol.as_str() {
                    "vless" => {
                        let mut params = Vec::new();
                        params.push(format!("security={}", security));
                        if security == "reality" {
                            let inbound_sni = stream
                                .reality_settings
                                .as_ref()
                                .and_then(|reality| reality.server_names.first().cloned())
                                .filter(|s| !Self::is_placeholder_sni(s));
                            // For Reality links prefer node-level SNI, because it reflects
                            // the currently applied node config after pin/rotation/sync.
                            let sni = node_sni
                                .clone()
                                .or(inbound_sni)
                                .unwrap_or_else(|| address.clone());
                            params.push(format!("sni={}", sni));
                            params.push(format!("pbk={}", reality_pub.clone().unwrap_or_default()));
                            if let Some(sid) = &short_id {
                                params.push(format!("sid={}", sid));
                            }
                            params.push("fp=chrome".to_string());
                        } else if security == "tls" {
                            let tls_sni = stream
                                .tls_settings
                                .as_ref()
                                .map(|t| t.server_name.clone())
                                .filter(|s| !Self::is_placeholder_sni(s));
                            let sni = tls_sni
                                .or_else(|| node_sni.clone())
                                .unwrap_or_else(|| address.clone());
                            params.push(format!("sni={}", sni));
                        }
                        params.push(format!("type={}", network));
                        if network == "tcp" {
                            params.push("headerType=none".to_string());
                            if security == "reality" {
                                params.push("flow=xtls-rprx-vision".to_string());
                            }
                        }
                        links.push(format!(
                            "vless://{}@{}:{}?{}#{}",
                            uuid,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "hysteria2" => {
                        let mut params = Vec::new();
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("insecure=1".to_string());

                        if let Ok(InboundType::Hysteria2(settings)) =
                            serde_json::from_str::<InboundType>(&inbound.settings)
                            && let Some(obfs) = settings.obfs
                            && obfs.ttype == "salamander"
                        {
                            params.push("obfs=salamander".to_string());
                            params.push(format!("obfs-password={}", obfs.password));
                        }

                        // Та же идентичность, что нода кладёт в
                        // Hysteria2User::password, и тот же построитель —
                        // см. services::user_tag.
                        let client_identity =
                            crate::services::user_tag::config_client_identity_for_user(
                                &self.pool,
                                sub.user_id,
                            )
                            .await?;
                        let auth =
                            crate::services::user_tag::proxy_auth_password(client_identity, &uuid);
                        links.push(format!(
                            "hysteria2://{}@{}:{}?{}#{}",
                            auth,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "trojan" => {
                        let mut params = Vec::new();
                        params.push("security=tls".to_string());
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("fp=chrome".to_string());
                        params.push(format!("type={}", network));
                        links.push(format!(
                            "trojan://{}@{}:{}?{}#{}",
                            uuid,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "tuic" => {
                        let mut params = Vec::new();
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));
                        params.push("alpn=h3".to_string());

                        let congestion = if let Ok(InboundType::Tuic(settings)) =
                            serde_json::from_str::<InboundType>(&inbound.settings)
                        {
                            settings.congestion_control
                        } else {
                            "cubic".to_string()
                        };
                        params.push(format!("congestion_control={}", congestion));
                        links.push(format!(
                            "tuic://{}:{}@{}:{}?{}#{}",
                            uuid,
                            uuid.replace("-", ""),
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    "naive" => {
                        let mut params = Vec::new();
                        let tls_sni = stream
                            .tls_settings
                            .as_ref()
                            .map(|t| t.server_name.clone())
                            .filter(|s| !Self::is_placeholder_sni(s));
                        let sni = tls_sni
                            .or_else(|| node_sni.clone())
                            .unwrap_or_else(|| address.clone());
                        params.push(format!("sni={}", sni));

                        if security == "reality" && stream.reality_settings.is_some() {
                            params.push(format!("pbk={}", reality_pub.clone().unwrap_or_default()));
                            if let Some(sid) = &short_id {
                                params.push(format!("sid={}", sid));
                            }
                        }

                        // Идентичность берётся тем же способом, что и для
                        // ноды. ВНИМАНИЕ: форма учётки naive расходится с
                        // конфигом ноды и БЕЗ NULL — нода пишет
                        // `username: user_{identity}` / `password: {uuid}`, а
                        // ссылка отдаёт `{identity}:{uuid}` (без префикса
                        // `user_` и с другим паролем). Это отдельный, более
                        // старый дефект: чинить его здесь значило бы менять
                        // формат живых ссылок, поэтому форма сохранена, а
                        // расхождение вынесено в отчёт.
                        let client_identity =
                            crate::services::user_tag::config_client_identity_for_user(
                                &self.pool,
                                sub.user_id,
                            )
                            .await?;
                        let auth =
                            crate::services::user_tag::proxy_auth_password(client_identity, &uuid);
                        links.push(format!(
                            "naive+https://{}@{}:{}?{}#{}",
                            auth,
                            address,
                            port,
                            params.join("&"),
                            encoded_remark
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(links)
    }

    /// Разрешает Telegram id (из тега соединения `user_{tg_id}`) в id активной
    /// подписки пользователя. Зеркалит цепочку tg_id → user → active
    /// subscription из учёта трафика (`api/v2/node.rs::heartbeat`): sing-box
    /// тегирует соединения Telegram id, а не id подписки.
    pub async fn get_active_subscription_id_by_tg_id(&self, tg_id: i64) -> Result<Option<i64>> {
        // Порядок выбора — общий для всей панели (subscription_repo::
        // ACTIVE_SUBSCRIPTION_ORDER_SQL). Прежний `expires_at DESC` всегда
        // выбирал бесплатную подписку с датой 9999 года, и весь учёт трафика с
        // kill-switch по этому tg_id уезжал не на ту подписку.
        sqlx::query_scalar(
            r#"
            SELECT s.id
            FROM subscriptions s
            JOIN users u ON u.id = s.user_id
            LEFT JOIN plans p ON p.id = s.plan_id
            WHERE u.tg_id = $1 AND s.status = 'active'
            ORDER BY COALESCE(p.is_free, FALSE) ASC, s.expires_at ASC, s.id ASC
            LIMIT 1
            "#,
        )
        .bind(tg_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to resolve active subscription by tg_id")
    }

    /// Возвращает идентичность клиента и vless_uuid подписки — для матчинга
    /// живых соединений: sing-box помечает их тегом `user_{identity}`, а в
    /// chains может присутствовать vless UUID (легаси-путь).
    ///
    /// Идентичность, а не сырой `u.tg_id`, по двум причинам. Во-первых, тег в
    /// конфиге ноды строится из `config_client_identity`, и матчинг обязан
    /// брать ровно то же значение, иначе kill-switch промахнётся мимо
    /// аккаунта без Telegram id. Во-вторых, non-optional декодирование
    /// nullable-колонки — это `Err(UnexpectedNull)`, то есть весь enforcement
    /// по такой подписке падал бы в ошибку.
    pub async fn get_subscription_connection_identity(
        &self,
        subscription_id: i64,
    ) -> Result<Option<(i64, Option<String>)>> {
        let row: Option<(Option<i64>, i64, Option<String>)> = sqlx::query_as(
            r#"
            SELECT u.tg_id, u.id, s.vless_uuid
            FROM subscriptions s
            JOIN users u ON u.id = s.user_id
            WHERE s.id = $1
            "#,
        )
        .bind(subscription_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to resolve subscription connection identity")?;

        Ok(row.map(|(tg_id, user_id, vless_uuid)| {
            (
                crate::services::user_tag::config_client_identity(tg_id, user_id),
                vless_uuid,
            )
        }))
    }

    /// Есть ли у владельца подписки ДРУГАЯ активная подписка. Нужно
    /// enforcement'у: sing-box тегирует соединения per-user (`user_{tg_id}`),
    /// поэтому «убить соединения подписки» по тегу означает убить ВСЕ
    /// соединения пользователя — включая обслуживаемые другой, легитимно
    /// активной (например, платной) подпиской.
    pub async fn user_has_other_active_subscription(&self, subscription_id: i64) -> Result<bool> {
        let exists: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM subscriptions other
                JOIN subscriptions target ON target.id = $1
                WHERE other.user_id = target.user_id
                  AND other.id <> target.id
                  AND other.status = 'active'
            )
            "#,
        )
        .bind(subscription_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to check for other active subscriptions")?;
        Ok(exists.unwrap_or(false))
    }

    /// Все привязки аккаунта. Инфраструктурные адреса (наши же ноды и
    /// фронтенды) отфильтрованы: они попадают в лизы, когда трафик идёт через
    /// релей, и устройством не являются.
    pub async fn list_user_devices(&self, user_id: i64) -> Result<Vec<UserDeviceLease>> {
        let cutoff = Utc::now() - Duration::days(DEVICE_LEASE_TTL_DAYS);
        sqlx::query_as::<_, UserDeviceLease>(
            r#"
            SELECT sdl.id,
                   sdl.subscription_id,
                   sdl.display_name,
                   sdl.device_name,
                   sdl.platform,
                   sdl.client_device_id,
                   sdl.user_agent,
                   sdl.last_ip,
                   sdl.first_seen_at,
                   sdl.last_seen_at,
                   (sdl.last_seen_at > NOW() - INTERVAL '15 minutes') AS online
            FROM subscription_device_leases sdl
            WHERE sdl.user_id = $1
              AND sdl.last_seen_at > $2
              AND sdl.last_ip <> '0.0.0.0'
              AND sdl.last_ip NOT IN (SELECT ip FROM nodes WHERE ip IS NOT NULL)
              AND sdl.last_ip NOT IN (
                  SELECT ip_address FROM frontend_servers WHERE ip_address IS NOT NULL)
            ORDER BY sdl.last_seen_at DESC
            "#,
        )
        .bind(user_id)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list device leases for the account")
    }

    /// Имя устройства, заданное человеком. Пустое имя сбрасывает на авто-имя.
    /// Возвращает false, если такой лизы у этого аккаунта нет.
    pub async fn rename_user_device(
        &self,
        user_id: i64,
        lease_id: i64,
        name: Option<&str>,
    ) -> Result<bool> {
        let cleaned: Option<String> = name.and_then(|raw| {
            let value: String = raw
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .chars()
                .take(32)
                .collect();
            if value.is_empty() { None } else { Some(value) }
        });

        let updated = sqlx::query(
            "UPDATE subscription_device_leases SET display_name = $1 \
             WHERE id = $2 AND user_id = $3",
        )
        .bind(cleaned.as_deref())
        .bind(lease_id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .context("Failed to rename a device lease")?;

        Ok(updated.rows_affected() > 0)
    }

    /// Отвязка устройства. Возвращает подписку, чьи соединения надо порвать,
    /// чтобы отвязанное устройство отвалилось сразу, а не на следующем опросе.
    pub async fn revoke_user_device(&self, user_id: i64, lease_id: i64) -> Result<Option<i64>> {
        let row: Option<(i64, String)> = sqlx::query_as(
            "DELETE FROM subscription_device_leases WHERE id = $1 AND user_id = $2 \
             RETURNING subscription_id, last_ip",
        )
        .bind(lease_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to revoke a device lease")?;

        let Some((subscription_id, last_ip)) = row else {
            return Ok(None);
        };

        // Легаси-трекинг адресов чистим следом: иначе устройство вернулось бы в
        // списки оттуда (get_active_ips падает на него, когда лиз не осталось).
        let _ = sqlx::query(
            "DELETE FROM subscription_ip_tracking WHERE subscription_id = $1 AND client_ip = $2",
        )
        .bind(subscription_id)
        .bind(&last_ip)
        .execute(&self.pool)
        .await;

        Ok(Some(subscription_id))
    }

    /// Отвязать все устройства аккаунта. Возвращает сколько удалено и подписки,
    /// чьи соединения надо порвать.
    pub async fn revoke_all_user_devices(&self, user_id: i64) -> Result<(u64, Vec<i64>)> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "DELETE FROM subscription_device_leases WHERE user_id = $1 \
             RETURNING subscription_id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to revoke all device leases")?;

        let mut subs: Vec<i64> = rows.into_iter().map(|(id,)| id).collect();
        let deleted = subs.len() as u64;
        subs.sort_unstable();
        subs.dedup();

        for sub_id in &subs {
            let _ = sqlx::query("DELETE FROM subscription_ip_tracking WHERE subscription_id = $1")
                .bind(sub_id)
                .execute(&self.pool)
                .await;
        }

        Ok((deleted, subs))
    }

    pub async fn get_subscription_device_limit(&self, subscription_id: i64) -> Result<i32> {
        let limit: Option<i32> = sqlx::query_scalar(
            "SELECT p.device_limit FROM subscriptions s JOIN plans p ON s.plan_id = p.id WHERE s.id = $1"
        )
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to fetch device limit")?;
        Ok(limit.unwrap_or(0))
    }

    pub async fn ensure_subscription_within_quota(&self, subscription_id: i64) -> Result<bool> {
        // `u.bonus_traffic_mb` joins in the user's one-off bonus allowance: the
        // ceiling is plan + bonus, never plan alone (see services/bonus_traffic.rs).
        let usage_row: Option<(i64, i32, String, bool, i32, i64)> = sqlx::query_as(
            r#"
            SELECT
                COALESCE(s.used_traffic, 0)::BIGINT AS used_traffic,
                COALESCE(p.traffic_limit_gb, 0)::INT AS traffic_limit_gb,
                COALESCE(s.status, 'pending') AS status,
                COALESCE(p.is_free, FALSE) AS is_free,
                COALESCE(p.daily_traffic_mb, 0)::INT AS daily_traffic_mb,
                COALESCE(u.bonus_traffic_mb, 0)::BIGINT AS bonus_traffic_mb
            FROM subscriptions s
            JOIN plans p ON p.id = s.plan_id
            JOIN users u ON u.id = s.user_id
            WHERE s.id = $1
            "#,
        )
        .bind(subscription_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch subscription quota")?;

        let Some((
            used_traffic,
            traffic_limit_gb,
            status,
            is_free,
            daily_traffic_mb,
            bonus_traffic_mb,
        )) = usage_row
        else {
            return Ok(false);
        };

        if status != "active" {
            return Ok(false);
        }

        // is_free и daily_traffic_mb передаются в предикат, а не только
        // выбирают статус ниже: у бесплатного плана потолок — суточная норма, а
        // не traffic_limit_gb. Если сюда уйдут не все четыре аргумента, эта
        // проверка разъедется с ночными свипами и подписка начнёт мигать между
        // 'active' и 'throttled'.
        if !crate::services::bonus_traffic::is_over_quota(
            used_traffic,
            is_free,
            traffic_limit_gb as i64,
            daily_traffic_mb as i64,
            bonus_traffic_mb,
        ) {
            return Ok(true);
        }

        // Бесплатные планы не «истекают» по трафику: 'throttled' снимается
        // суточным пополнением, 'expired' пришлось бы чинить руками.
        // 'throttled' допустим только при daily_traffic_mb > 0 — без суточного
        // пополнения из него нет автоматического выхода, поэтому бесплатный
        // план без пополнения истекает как платный (админ снимет через extend).
        let over_quota_status = if is_free && daily_traffic_mb > 0 {
            "throttled"
        } else {
            "expired"
        };
        let _ = sqlx::query("UPDATE subscriptions SET status = $1 WHERE id = $2")
            .bind(over_quota_status)
            .bind(subscription_id)
            .execute(&self.pool)
            .await;
        Ok(false)
    }

    pub async fn expire_over_quota_subscriptions(&self) -> Result<Vec<ExpiredQuotaSubscription>> {
        // Бесплатные планы (is_free = TRUE) не переводятся в 'expired' при исчерпании трафика.
        // Для них просто убиваем соединения — daily_traffic_topup восстановит трафик на следующий день.
        // Остальные планы переводятся в 'expired' как обычно.
        // The ceiling is plan allowance + the user's bonus traffic — see
        // `bonus_traffic::QUOTA_LIMIT_BYTES_SQL`, which every gate shares so
        // none of them can forget the bonus term.
        let sql = format!(
            r#"
            UPDATE subscriptions s
            SET status = 'expired'
            FROM plans p, users u
            WHERE s.plan_id = p.id
              AND u.id = s.user_id
              AND s.status = 'active'
              AND COALESCE(p.is_free, FALSE) = FALSE
              AND COALESCE(p.traffic_limit_gb, 0) > 0
              AND COALESCE(s.used_traffic, 0) >= {limit}
            RETURNING s.id AS subscription_id, s.user_id, s.node_id, s.plan_id
            "#,
            limit = crate::services::bonus_traffic::QUOTA_LIMIT_BYTES_SQL,
        );
        let rows = sqlx::query_as::<_, ExpiredQuotaSubscription>(&sql)
            .fetch_all(&self.pool)
            .await
            .context("Failed to expire subscriptions over traffic quota")?;

        Ok(rows)
    }

    /// Переводит подписки бесплатных планов, исчерпавшие трафик, в статус
    /// 'throttled'. В отличие от 'expired', это временная блокировка: подписка
    /// исключается из конфигов нод (get_active_subs_by_plans отбирает только
    /// 'active'), а суточное пополнение (daily_traffic_topup) вернёт её в
    /// 'active', как только used_traffic снова окажется ниже лимита.
    ///
    /// Троттлим ТОЛЬКО планы с daily_traffic_mb > 0: единственный
    /// автоматический путь назад — суточное пополнение, и бесплатный план без
    /// него (daily_traffic_mb = 0/NULL) застрял бы в 'throttled' навсегда.
    /// Такие планы остаются 'active' (как до введения троттлинга).
    pub async fn throttle_free_quota_subscriptions(&self) -> Result<Vec<ExpiredQuotaSubscription>> {
        // Bonus traffic counts here too: a free-plan user who was given extra
        // MB must keep connecting until the plan's daily quota AND the bonus
        // are gone, otherwise the grant would be invisible to them.
        //
        // {limited} — «у плана вообще есть потолок» — вынесен в общую константу
        // и разделяется с ночным снятием троттлинга (monitoring::
        // daily_traffic_topup). Раньше здесь стояли два условия > 0, а там —
        // своё представление о безлимите; расхождение и есть тот самый флап.
        let sql = format!(
            r#"
            UPDATE subscriptions s
            SET status = 'throttled'
            FROM plans p, users u
            WHERE s.plan_id = p.id
              AND u.id = s.user_id
              AND s.status = 'active'
              AND COALESCE(p.is_free, FALSE) = TRUE
              AND {limited}
              AND COALESCE(s.used_traffic, 0) >= {limit}
            RETURNING s.id AS subscription_id, s.user_id, s.node_id, s.plan_id
            "#,
            limited = crate::services::bonus_traffic::QUOTA_LIMITED_PLAN_SQL,
            limit = crate::services::bonus_traffic::QUOTA_LIMIT_BYTES_SQL,
        );
        let rows = sqlx::query_as::<_, ExpiredQuotaSubscription>(&sql)
            .fetch_all(&self.pool)
            .await
            .context("Failed to throttle free subscriptions over quota")?;

        Ok(rows)
    }

    pub async fn expire_over_quota_candidates(
        &self,
        subscription_ids: &[i64],
    ) -> Result<Vec<ExpiredQuotaSubscription>> {
        if subscription_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Бесплатные планы не истекают по трафику — пропускаем через is_free = FALSE.
        // Потолок = лимит плана + бонусный трафик пользователя (bonus_traffic.rs);
        // это тот же путь, которым heartbeat из api/v2/node.rs проверяет квоту.
        let sql = format!(
            r#"
            UPDATE subscriptions s
            SET status = 'expired'
            FROM plans p, users u
            WHERE s.plan_id = p.id
              AND u.id = s.user_id
              AND s.id = ANY($1)
              AND s.status = 'active'
              AND COALESCE(p.is_free, FALSE) = FALSE
              AND COALESCE(p.traffic_limit_gb, 0) > 0
              AND COALESCE(s.used_traffic, 0) >= {limit}
            RETURNING s.id AS subscription_id, s.user_id, s.node_id, s.plan_id
            "#,
            limit = crate::services::bonus_traffic::QUOTA_LIMIT_BYTES_SQL,
        );
        let rows = sqlx::query_as::<_, ExpiredQuotaSubscription>(&sql)
            .bind(subscription_ids)
            .fetch_all(&self.pool)
            .await
            .context("Failed to expire candidate subscriptions over traffic quota")?;

        Ok(rows)
    }

    pub async fn update_ips(&self, subscription_id: i64, ip_list: Vec<String>) -> Result<()> {
        let normalized: Vec<String> = ip_list
            .into_iter()
            .filter_map(|ip| Self::normalize_client_ip(&ip))
            .collect();

        caramba_db::repositories::subscription_repo::SubscriptionRepository::new(self.pool.clone())
            .update_ips(subscription_id, normalized.clone())
            .await?;

        let mut dedup = HashSet::new();
        for ip in normalized {
            if !dedup.insert(ip.clone()) {
                continue;
            }
            // Хартбит ноды: ни User-Agent, ни заголовков приложения тут нет,
            // поэтому устройство может быть только освежено по адресу.
            if let Err(e) = self
                .upsert_device_lease(subscription_id, &ip, None, None, &DeviceIdentity::default())
                .await
            {
                warn!(
                    "Failed to refresh device lease from node connection for sub {} ip {}: {}",
                    subscription_id, ip, e
                );
            }
        }

        Ok(())
    }

    pub async fn get_active_ips(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<SubscriptionIpTracking>> {
        let cutoff = Utc::now() - Duration::minutes(15);
        let mut rows = match sqlx::query_as::<_, SubscriptionIpTracking>(
            r#"
            SELECT
                sdl.id,
                sdl.subscription_id,
                sdl.last_ip AS client_ip,
                COALESCE(NULLIF(sdl.device_name, ''), sdl.user_agent) AS user_agent,
                sdl.last_seen_at
            FROM subscription_device_leases sdl
            WHERE sdl.subscription_id = $1
              AND sdl.last_seen_at > $2
              AND sdl.last_ip <> '0.0.0.0'
            ORDER BY sdl.last_seen_at DESC
            "#,
        )
        .bind(subscription_id)
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                warn!(
                    "Device lease query failed for sub {} (falling back to legacy tracking): {}",
                    subscription_id, e
                );
                Vec::new()
            }
        };

        if rows.is_empty() {
            rows = self.get_active_ips_legacy(subscription_id, cutoff).await?;
        }

        let infra_ips = self.infrastructure_ips().await;
        let filtered = rows
            .into_iter()
            .filter(|row| Self::should_track_device_ip(&row.client_ip, &infra_ips))
            .collect::<Vec<_>>();

        Ok(filtered)
    }

    pub async fn cleanup_old_ip_tracking(&self) -> Result<u64> {
        // Легаси-трекинг адресов живёт час: это журнал обращений, а не привязка.
        let cutoff = Utc::now() - Duration::hours(1);
        let result = sqlx::query("DELETE FROM subscription_ip_tracking WHERE last_seen_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        let mut affected = result.rows_affected();

        // А вот привязка устройства — не журнал: по постановке она держится до
        // ручной отвязки. Прежний часовой срок означал, что телефон, которым не
        // пользовались вечер, наутро заводился заново и съедал слот лимита;
        // список устройств в кабинете при этом показывал пустоту. Окно здесь то
        // же, по которому считается лимит (DEVICE_LEASE_TTL_DAYS), иначе гейт и
        // уборщик снова разошлись бы.
        let lease_cutoff = Utc::now() - Duration::days(DEVICE_LEASE_TTL_DAYS);
        match sqlx::query("DELETE FROM subscription_device_leases WHERE last_seen_at < $1")
            .bind(lease_cutoff)
            .execute(&self.pool)
            .await
        {
            Ok(r) => affected += r.rows_affected(),
            Err(e) => warn!("Failed to cleanup device leases: {}", e),
        }

        Ok(affected)
    }

    pub async fn remove_tracked_ips(
        &self,
        subscription_id: i64,
        ip_list: &[String],
    ) -> Result<u64> {
        if ip_list.is_empty() {
            return Ok(0);
        }

        let result = sqlx::query(
            "DELETE FROM subscription_ip_tracking WHERE subscription_id = $1 AND client_ip = ANY($2)",
        )
        .bind(subscription_id)
        .bind(ip_list)
        .execute(&self.pool)
        .await
        .context("Failed to delete tracked IPs")?;

        let mut affected = result.rows_affected();

        match sqlx::query(
            "DELETE FROM subscription_device_leases WHERE subscription_id = $1 AND last_ip = ANY($2)",
        )
        .bind(subscription_id)
        .bind(ip_list)
        .execute(&self.pool)
        .await
        {
            Ok(r) => affected += r.rows_affected(),
            Err(e) => warn!(
                "Failed to delete matching device lease rows for sub {}: {}",
                subscription_id, e
            ),
        }

        Ok(affected)
    }

    pub async fn get_subscription_by_uuid(&self, uuid: &str) -> Result<Subscription> {
        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE subscription_uuid = $1",
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        Ok(sub)
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<Subscription>> {
        let sub = sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch subscription by ID")?;
        Ok(sub)
    }

    /// Имя устройства из User-Agent: «клиент · платформа». Современные клиенты
    /// пишут платформу в скобках («HiddifyNext/4.0.0 (ios) …»), а слова
    /// «iphone» там нет — прежний разбор отдавал безликий «Sing-box Client».
    pub fn parse_device_name(&self, ua: &str) -> String {
        let l = ua.to_lowercase();

        let client = [
            ("hiddify", "Hiddify"),
            ("happ", "Happ"),
            ("v2raytun", "v2rayTun"),
            ("streisand", "Streisand"),
            ("shadowrocket", "Shadowrocket"),
            ("clash-verge", "Clash Verge"),
            ("clash verge", "Clash Verge"),
            ("nekobox", "NekoBox"),
            ("nekoray", "NekoRay"),
            ("v2rayng", "v2rayNG"),
            ("v2rayn", "v2rayN"),
            ("karing", "Karing"),
            ("stash", "Stash"),
            ("mihomo", "Mihomo"),
            ("clash", "Clash"),
            ("sing-box", "sing-box"),
            ("singbox", "sing-box"),
            ("xray", "Xray"),
            ("v2ray", "V2Ray"),
        ]
        .iter()
        .find(|(needle, _)| l.contains(needle))
        .map(|(_, label)| *label);

        let platform = if l.contains("iphone") || l.contains("(ios") || l.contains(" ios") {
            Some("iPhone")
        } else if l.contains("ipad") {
            Some("iPad")
        } else if l.contains("android") {
            Some("Android")
        } else if l.contains("windows") {
            Some("Windows")
        } else if l.contains("macos")
            || l.contains("mac os")
            || l.contains("macintosh")
            || l.contains("darwin")
        {
            Some("Mac")
        } else if l.contains("linux") {
            Some("Linux")
        } else {
            None
        };

        match (client, platform) {
            (Some(c), Some(p)) => format!("{} · {}", c, p),
            (Some(c), None) => c.to_string(),
            (None, Some(p)) => p.to_string(),
            (None, None) => "Device".to_string(),
        }
    }

    pub fn detect_client_type(&self, ua: Option<&str>) -> String {
        let ua = match ua {
            Some(s) => s.to_lowercase(),
            None => return "html".to_string(),
        };

        if ua.contains("hiddify") || ua.contains("sing-box") {
            "singbox".to_string()
        } else if ua.contains("clash") || ua.contains("stash") {
            "clash".to_string()
        } else if ua.contains("v2ray")
            || ua.contains("xray")
            || ua.contains("fair")
            || ua.contains("shadowrocket")
            || ua.contains("happ")
        {
            "v2ray".to_string()
        } else if ua.contains("mozilla") || ua.contains("chrome") || ua.contains("safari") {
            "html".to_string()
        } else {
            "singbox".to_string()
        }
    }

    pub async fn track_access(
        &self,
        sub_id: i64,
        ip: &str,
        user_agent: Option<&str>,
        device: &DeviceIdentity,
    ) -> Result<()> {
        // Адрес годится для учёта устройств только если он вообще разобрался и не
        // принадлежит нашей же инфраструктуре. Инфраструктурный адрес означает, что
        // край (Caddy/фронтенд-узел) не проставил X-Forwarded-For и до нас доехал
        // адрес самого прокси, а не клиента.
        let attributable_ip = match Self::normalize_client_ip(ip) {
            Some(candidate) => {
                let infra_ips = self.infrastructure_ips().await;
                let is_infra = Self::parse_ip_maybe(&candidate)
                    .is_some_and(|parsed| infra_ips.contains(&parsed));
                if is_infra { None } else { Some(candidate) }
            }
            None => None,
        };

        if attributable_ip.is_none() && Self::should_warn_unattributed_ip(sub_id) {
            warn!(
                "Subscription {} fetched from unattributable IP {:?}: the edge did not forward \
                 the client address (X-Forwarded-For / CF-Connecting-IP). Access time is recorded, \
                 but device tracking and device limits stay blind for this subscription.",
                sub_id, ip
            );
        }

        // Факт обращения фиксируем всегда: подписку действительно скачали. Прежний
        // last_access_ip сохраняем через COALESCE, чтобы неатрибутируемый запрос не
        // стирал последний известный адрес клиента.
        sqlx::query(
            "UPDATE subscriptions SET last_sub_access = $1, last_access_ip = COALESCE($2, last_access_ip), last_access_ua = $3 WHERE id = $4"
        )
        .bind(Utc::now())
        .bind(attributable_ip.as_deref())
        .bind(user_agent)
        .bind(sub_id)
        .execute(&self.pool)
        .await?;

        let Some(normalized_ip) = attributable_ip else {
            return Ok(());
        };

        let ua = user_agent.unwrap_or("");
        let device_name = self.parse_device_name(ua);

        sqlx::query(
            "INSERT INTO subscription_ip_tracking (subscription_id, client_ip, user_agent, last_seen_at) 
             VALUES ($1, $2, $3, CURRENT_TIMESTAMP)
             ON CONFLICT(subscription_id, client_ip) DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP, user_agent = excluded.user_agent"
        )
        .bind(sub_id)
        .bind(&normalized_ip)
        .bind(device_name)
        .execute(&self.pool)
        .await?;

        if let Err(e) = self
            .upsert_device_lease(sub_id, &normalized_ip, user_agent, None, device)
            .await
        {
            warn!(
                "Failed to upsert device lease for subscription {} (ip {}): {}",
                sub_id, normalized_ip, e
            );
        }

        Ok(())
    }

    pub async fn get_active_nodes_for_config(&self) -> Result<Vec<NodeInfo>> {
        let node_repo = NodeRepository::new(self.pool.clone());
        let nodes = node_repo
            .get_all_nodes()
            .await?
            .into_iter()
            .filter(|n| n.is_enabled && n.status == "active")
            .collect::<Vec<Node>>();

        let node_ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        let inbounds_map = if node_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let inbounds = sqlx::query_as::<_, caramba_db::models::network::Inbound>(
                "SELECT * FROM inbounds WHERE enable = TRUE AND node_id = ANY($1)",
            )
            .bind(&node_ids)
            .fetch_all(&self.pool)
            .await?;

            let mut map = std::collections::HashMap::new();
            for inbound in inbounds {
                map.entry(inbound.node_id)
                    .or_insert_with(Vec::new)
                    .push(inbound);
            }
            map
        };

        let node_infos = nodes
            .iter()
            .map(|n| {
                let node_inbounds = inbounds_map.get(&n.id).cloned().unwrap_or_default();
                NodeInfo::new(n, node_inbounds)
            })
            .collect();

        Ok(node_infos)
    }

    pub async fn get_node_infos_with_relays(&self, nodes: &[Node]) -> Result<Vec<NodeInfo>> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        let node_ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        let inbounds_map = self.fetch_inbounds_for_nodes(&node_ids).await?;

        let relay_ids: Vec<i64> = nodes
            .iter()
            .filter_map(|n| n.relay_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let relays_map = if relay_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let node_repo = NodeRepository::new(self.pool.clone());
            let mut relay_nodes = Vec::new();
            for relay_id in &relay_ids {
                if let Some(node) = node_repo.get_node_by_id(*relay_id).await? {
                    relay_nodes.push(node);
                }
            }
            let relay_inbounds_map = self.fetch_inbounds_for_nodes(&relay_ids).await?;

            let mut map = std::collections::HashMap::new();
            for r in relay_nodes {
                let r_id = r.id;
                let r_inbounds = relay_inbounds_map.get(&r_id).cloned().unwrap_or_default();
                map.insert(r_id, NodeInfo::new(&r, r_inbounds));
            }
            map
        };

        let mut node_infos = Vec::new();
        for n in nodes {
            let n_inbounds = inbounds_map.get(&n.id).cloned().unwrap_or_default();
            let mut ni = NodeInfo::new(n, n_inbounds);

            if let Some(r_id) = n.relay_id
                && let Some(r_info) = relays_map.get(&r_id)
            {
                ni.relay_info = Some(Box::new(r_info.clone()));
            }
            node_infos.push(ni);
        }

        Ok(node_infos)
    }

    /// Fetch all active relay nodes with their inbounds — used to auto-match
    /// relay chains to every exit node at config generation time.
    pub async fn get_all_active_relay_infos(&self) -> Result<Vec<NodeInfo>> {
        let relay_nodes: Vec<Node> =
            sqlx::query_as("SELECT * FROM nodes WHERE is_relay = TRUE AND status = 'active'")
                .fetch_all(&self.pool)
                .await?;

        if relay_nodes.is_empty() {
            return Ok(Vec::new());
        }

        let relay_ids: Vec<i64> = relay_nodes.iter().map(|n| n.id).collect();
        let inbounds_map = self.fetch_inbounds_for_nodes(&relay_ids).await?;

        let result = relay_nodes
            .iter()
            .map(|node| {
                let inbounds = inbounds_map.get(&node.id).cloned().unwrap_or_default();
                NodeInfo::new(node, inbounds)
            })
            .collect();

        Ok(result)
    }

    async fn fetch_inbounds_for_nodes(
        &self,
        node_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, Vec<caramba_db::models::network::Inbound>>> {
        if node_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let inbounds = sqlx::query_as::<_, caramba_db::models::network::Inbound>(&format!(
            "{} WHERE enable = TRUE AND node_id = ANY($1)",
            Self::INBOUND_SELECT_SQL
        ))
        .bind(node_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut map = std::collections::HashMap::new();
        for inbound in inbounds {
            map.entry(inbound.node_id)
                .or_insert_with(Vec::new)
                .push(inbound);
        }
        Ok(map)
    }

    pub async fn get_user_keys(&self, sub: &Subscription) -> Result<UserKeys> {
        let user_uuid = Self::subscription_user_uuid(sub).ok_or_else(|| {
            anyhow::anyhow!(
                "No UUID available for subscription {} (vless_uuid/subscription_uuid missing)",
                sub.id
            )
        })?;

        // Раньше здесь `SELECT tg_id` декодировался в non-optional i64:
        // у email-аккаунта sqlx возвращал Err(UnexpectedNull), `?` вылетал
        // наружу, и GET /sub/{uuid} превращался в 500 — стоявший ниже
        // `.unwrap_or(0)` до этого места просто не доживал.
        let client_identity =
            crate::services::user_tag::config_client_identity_for_user(&self.pool, sub.user_id)
                .await?;

        let hy2_password =
            crate::services::user_tag::proxy_auth_password(client_identity, &user_uuid);
        let awg_private_key = self.derive_awg_key(&user_uuid);

        Ok(UserKeys {
            user_uuid,
            hy2_password,
            _awg_private_key: Some(awg_private_key.clone()),
        })
    }

    fn derive_awg_key(&self, uuid: &str) -> String {
        Self::generate_amneziawg_key(uuid)
    }

    pub async fn check_and_send_alerts(&self) -> Result<Vec<(i64, AlertType)>> {
        use caramba_db::models::store::AlertType;
        let mut alerts_to_send = vec![];

        // Traffic alerts (80%, 90%, 100%)
        // Сразу включаем traffic_limit_gb в запрос чтобы не делать N+1 запросов
        let subs = sqlx::query_as::<_, (i64, i64, i64, i32, String)>(
            "SELECT s.id, s.user_id, s.used_traffic, p.traffic_limit_gb, COALESCE(s.alerts_sent, '[]')
             FROM subscriptions s
             JOIN plans p ON s.plan_id = p.id
             WHERE s.status = 'active' AND p.traffic_limit_gb > 0",
        )
        .fetch_all(&self.pool)
        .await?;

        for (sub_id, user_id, used_traffic_bytes, traffic_limit_gb, alerts_json) in subs {
            if traffic_limit_gb == 0 {
                continue;
            }

            let total_traffic_bytes = traffic_limit_gb as i64 * 1024 * 1024 * 1024;
            let percentage = (used_traffic_bytes as f64 / total_traffic_bytes as f64) * 100.0;

            let mut alerts: Vec<String> = serde_json::from_str(&alerts_json).unwrap_or_default();
            let prev_len = alerts.len();

            // Check 80% threshold
            if percentage >= 80.0 && !alerts.contains(&"80_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic80));
                alerts.push("80_percent".to_string());
            }

            // Check 90% threshold
            if percentage >= 90.0 && !alerts.contains(&"90_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::Traffic90));
                alerts.push("90_percent".to_string());
            }

            // Check 100% threshold — трафик исчерпан, доступ будет приостановлен
            if percentage >= 100.0 && !alerts.contains(&"100_percent".to_string()) {
                alerts_to_send.push((user_id, AlertType::TrafficExceeded));
                alerts.push("100_percent".to_string());
            }

            // Обновляем alerts_sent только если добавились новые метки — избегаем лишних записей в БД
            if alerts.len() > prev_len {
                let alerts_json = serde_json::to_string(&alerts)?;
                sqlx::query("UPDATE subscriptions SET alerts_sent = $1 WHERE id = $2")
                    .bind(&alerts_json)
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await?;
            }
        }

        // Expiry alerts (3 days before)
        let expiring_subs = sqlx::query_as::<_, (i64, i64, String)>(
            "SELECT s.id, s.user_id, COALESCE(s.alerts_sent, '[]')
             FROM subscriptions s
             WHERE s.status = 'active'
             AND s.expires_at BETWEEN CURRENT_TIMESTAMP + interval '2 days' AND CURRENT_TIMESTAMP + interval '3 days'"
        )
        .fetch_all(&self.pool)
        .await?;

        for (sub_id, user_id, alerts_json) in expiring_subs {
            let mut alerts: Vec<String> = serde_json::from_str(&alerts_json).unwrap_or_default();
            if !alerts.contains(&"expiry_3d".to_string()) {
                alerts_to_send.push((user_id, AlertType::Expiry3Days));
                // Фиксируем метку чтобы не отправлять уведомление повторно при следующем прогоне
                alerts.push("expiry_3d".to_string());
                let updated_json = serde_json::to_string(&alerts)?;
                let _ = sqlx::query("UPDATE subscriptions SET alerts_sent = $1 WHERE id = $2")
                    .bind(&updated_json)
                    .bind(sub_id)
                    .execute(&self.pool)
                    .await;
            }
        }

        Ok(alerts_to_send)
    }

    pub fn generate_clash(
        &self,
        sub: &Subscription,
        nodes: &[NodeInfo],
        keys: &UserKeys,
        relay_nodes: &[NodeInfo],
    ) -> Result<String> {
        crate::singbox::subscription_generator::generate_clash_config(sub, nodes, keys, relay_nodes)
    }

    pub fn generate_v2ray(
        &self,
        sub: &Subscription,
        nodes: &[NodeInfo],
        keys: &UserKeys,
        relay_nodes: &[NodeInfo],
    ) -> Result<String> {
        crate::singbox::subscription_generator::generate_v2ray_config(sub, nodes, keys, relay_nodes)
    }

    pub fn generate_singbox(
        &self,
        sub: &Subscription,
        nodes: &[NodeInfo],
        keys: &UserKeys,
        variant: Option<&str>,
        relay_nodes: &[NodeInfo],
    ) -> Result<String> {
        let config = crate::singbox::subscription_generator::generate_singbox_config(
            sub,
            nodes,
            keys,
            relay_nodes,
        )?;

        match variant {
            Some(variant_id) => apply_connection_variant(&config, variant_id),
            None => Ok(config),
        }
    }

    pub fn get_singbox_connection_variants(&self) -> Vec<SingboxConnectionVariant> {
        fixed_connection_variants()
    }

    pub async fn update_subscription_node(&self, sub_id: i64, node_id: Option<i64>) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET node_id = $1 WHERE id = $2")
            .bind(node_id)
            .bind(sub_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    fn generate_amneziawg_key(uuid: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(uuid.as_bytes());
        hasher.update(b"amneziawg-key-salt");
        let result = hasher.finalize();

        let mut key = [0u8; 32];
        key.copy_from_slice(&result[..32]);

        key[0] &= 248;
        key[31] &= 127;
        key[31] |= 64;

        base64::Engine::encode(&base64::prelude::BASE64_STANDARD, key)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_SUBSCRIPTION_USER_AGENT, DEVICE_MERGE_WINDOW_MINUTES, DeviceAdmission, DeviceIdentity,
        Duration, LeaseCandidate, SubscriptionService, Utc, merge_lease_candidate,
    };
    use axum::http::HeaderMap;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                axum::http::HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    /// Тот самый баг: отпечаток считался от subscription_id, поэтому смена
    /// тарифа делала все телефоны человека новыми устройствами и упирала его в
    /// лимит на его же технике. Отпечаток обязан зависеть только от того, что
    /// у устройства и аккаунта, а строка подписки в него входить не может.
    #[test]
    fn the_fingerprint_survives_a_plan_change() {
        let before = SubscriptionService::device_fingerprint_for(
            42,
            Some("11111111-2222-3333-4444-555555555555"),
            Some("CarambaConnect/1.0.0"),
        );
        // Приложение обновилось: User-Agent другой, устройство то же.
        let after = SubscriptionService::device_fingerprint_for(
            42,
            Some("11111111-2222-3333-4444-555555555555"),
            Some("CarambaConnect/1.1.0"),
        );
        assert_eq!(
            before, after,
            "идентификатор установки обязан перевешивать User-Agent"
        );
    }

    #[test]
    fn different_installations_are_different_devices() {
        let first = SubscriptionService::device_fingerprint_for(42, Some("aaa"), None);
        let second = SubscriptionService::device_fingerprint_for(42, Some("bbb"), None);
        assert_ne!(first, second);
    }

    /// Сторонний клиент своего идентификатора прислать не может: для него
    /// отпечаток считается от владельца и User-Agent. Ключевое здесь — владелец,
    /// а не подписка.
    #[test]
    fn third_party_clients_fall_back_to_the_user_agent() {
        let hiddify = SubscriptionService::device_fingerprint_for(42, None, Some("Hiddify/2.0"));
        let clash = SubscriptionService::device_fingerprint_for(42, None, Some("clash-verge/1.5"));
        assert_ne!(hiddify, clash);

        // Один и тот же клиент у ДРУГОГО человека — другое устройство.
        let other_user = SubscriptionService::device_fingerprint_for(43, None, Some("Hiddify/2.0"));
        assert_ne!(hiddify, other_user);

        // Клиент, не приславший вообще ничего, всё равно получает отпечаток:
        // иначе хартбит без UA заводил бы каждый раз новую строку.
        let nameless_a = SubscriptionService::device_fingerprint_for(42, None, None);
        let nameless_b = SubscriptionService::device_fingerprint_for(42, None, None);
        assert_eq!(nameless_a, nameless_b);
    }

    #[test]
    fn device_headers_are_read_and_cleaned() {
        let ident = DeviceIdentity::from_headers(&headers(&[
            (DeviceIdentity::HEADER_ID, "  device-1  "),
            (DeviceIdentity::HEADER_NAME, " Pixel 8 "),
            (DeviceIdentity::HEADER_PLATFORM, "Android"),
        ]));
        assert_eq!(ident.client_device_id.as_deref(), Some("device-1"));
        assert_eq!(ident.display_name.as_deref(), Some("Pixel 8"));
        // Платформа приводится к нижнему регистру: она ключ, а не текст для
        // человека.
        assert_eq!(ident.platform.as_deref(), Some("android"));
        assert!(!ident.is_anonymous());
    }

    /// Заголовки приходят из интернета и попадают в имя устройства в кабинете:
    /// пустое значение не должно превращаться в пустое имя, а длинное — в
    /// растянутую строку.
    #[test]
    fn empty_and_oversized_headers_are_refused() {
        let ident = DeviceIdentity::from_headers(&headers(&[
            (DeviceIdentity::HEADER_ID, "   "),
            (DeviceIdentity::HEADER_NAME, &"x".repeat(200)),
        ]));
        assert_eq!(ident.client_device_id, None);
        assert!(ident.is_anonymous());
        assert_eq!(ident.display_name.as_deref().map(str::len), Some(64));
    }

    #[test]
    fn a_client_without_headers_is_anonymous() {
        let ident = DeviceIdentity::from_headers(&headers(&[]));
        assert_eq!(ident, DeviceIdentity::default());
        assert!(ident.is_anonymous());
    }

    /// Лимит гейтит ТОЛЬКО новое устройство. Уже привязанное проходит всегда:
    /// иначе сужение тарифа молча отрубало бы человеку телефон, которым он
    /// пользуется, вместо того чтобы не пустить следующий.
    #[test]
    fn the_limit_only_gates_new_devices() {
        let known = DeviceAdmission {
            known: true,
            used: 9,
            limit: 3,
        };
        assert!(known.allowed());

        let fresh_within_limit = DeviceAdmission {
            known: false,
            used: 2,
            limit: 3,
        };
        assert!(fresh_within_limit.allowed());

        let fresh_at_limit = DeviceAdmission {
            known: false,
            used: 3,
            limit: 3,
        };
        assert!(!fresh_at_limit.allowed());

        // limit = 0 означает безлимит, а не «ноль устройств».
        let unlimited = DeviceAdmission {
            known: false,
            used: 100,
            limit: 0,
        };
        assert!(unlimited.allowed());
    }

    // ---------------------------------------------------------- слияние лиз

    fn app_device() -> DeviceIdentity {
        DeviceIdentity {
            client_device_id: Some("dev-1".to_string()),
            display_name: Some("Pixel 8".to_string()),
            platform: Some("android".to_string()),
        }
    }

    fn lease(
        id: i64,
        client_device_id: Option<&str>,
        user_agent: Option<&str>,
        ip: &str,
        age_minutes: i64,
    ) -> LeaseCandidate {
        LeaseCandidate {
            id,
            client_device_id: client_device_id.map(ToOwned::to_owned),
            user_agent: user_agent.map(ToOwned::to_owned),
            last_ip: ip.to_string(),
            last_seen_at: Utc::now() - Duration::minutes(age_minutes),
        }
    }

    /// Тот самый баг. Приложение уже завело лизу по `X-Caramba-Device-Id`, а
    /// конфиг подписки качает Go-ядро, и до этой правки оно представлялось
    /// одним User-Agent. Два представления одного телефона занимали два слота
    /// лимита устройств.
    #[test]
    fn the_core_fetch_lands_in_the_lease_the_app_already_opened() {
        let now = Utc::now();
        let rows = vec![lease(
            7,
            Some("dev-1"),
            Some("CarambaConnect/1.0"),
            "1.2.3.4",
            2,
        )];
        let matched = merge_lease_candidate(
            &DeviceIdentity::default(),
            Some(CORE_SUBSCRIPTION_USER_AGENT),
            "1.2.3.4",
            now,
            &rows,
        )
        .expect("анонимная выборка ядра обязана попасть в лизу приложения");
        assert_eq!(matched.id, 7);
        // Идентификатор берётся из НАЙДЕННОЙ строки: по нему потом считается её
        // отпечаток, и терять его нельзя.
        assert_eq!(matched.client_device_id.as_deref(), Some("dev-1"));
    }

    /// Обратное направление: ядро успело первым, лиза безымянная. Запрос,
    /// назвавший себя, обязан присвоить ей идентификатор, а не завести вторую.
    #[test]
    fn a_named_request_adopts_the_anonymous_lease_of_the_same_client() {
        let now = Utc::now();
        let rows = vec![lease(
            9,
            None,
            Some(CORE_SUBSCRIPTION_USER_AGENT),
            "1.2.3.4",
            1,
        )];
        let matched = merge_lease_candidate(
            &app_device(),
            Some(CORE_SUBSCRIPTION_USER_AGENT),
            "1.2.3.4",
            now,
            &rows,
        )
        .expect("именованный запрос обязан усыновить безымянную лизу того же клиента");
        assert_eq!(matched.id, 9);
        assert_eq!(matched.client_device_id.as_deref(), Some("dev-1"));
    }

    /// Чужой клиент за тем же NAT — ДРУГОЕ устройство. Иначе починка двойной
    /// лизы открыла бы дыру в лимите: Hiddify на ноутбуке ехал бы на лизе
    /// телефона и не занимал слот.
    #[test]
    fn a_third_party_client_never_merges_into_someone_elses_lease() {
        let now = Utc::now();
        let rows = vec![lease(
            7,
            Some("dev-1"),
            Some("CarambaConnect/1.0"),
            "1.2.3.4",
            2,
        )];
        assert_eq!(
            merge_lease_candidate(
                &DeviceIdentity::default(),
                Some("Hiddify/2.0"),
                "1.2.3.4",
                now,
                &rows,
            ),
            None
        );
    }

    /// Границы склейки: другой адрес, протухшее окно и адрес-заглушка
    /// внутренних вызовов не склеивают ничего.
    #[test]
    fn the_merge_is_bounded_by_address_and_time() {
        let now = Utc::now();
        let other_ip = vec![lease(7, Some("dev-1"), None, "5.6.7.8", 1)];
        assert_eq!(
            merge_lease_candidate(
                &DeviceIdentity::default(),
                Some(CORE_SUBSCRIPTION_USER_AGENT),
                "1.2.3.4",
                now,
                &other_ip,
            ),
            None
        );

        let stale = vec![lease(
            7,
            Some("dev-1"),
            None,
            "1.2.3.4",
            DEVICE_MERGE_WINDOW_MINUTES + 1,
        )];
        assert_eq!(
            merge_lease_candidate(
                &DeviceIdentity::default(),
                Some(CORE_SUBSCRIPTION_USER_AGENT),
                "1.2.3.4",
                now,
                &stale,
            ),
            None
        );

        let fresh = vec![lease(7, Some("dev-1"), None, "0.0.0.0", 1)];
        assert_eq!(
            merge_lease_candidate(
                &DeviceIdentity::default(),
                Some(CORE_SUBSCRIPTION_USER_AGENT),
                "0.0.0.0",
                now,
                &fresh,
            ),
            None
        );
    }

    /// Именованный запрос не усыновляет безымянную лизу ДРУГОГО клиента: у
    /// человека за одним адресом может стоять и наш телефон, и сторонний
    /// клиент на компьютере.
    #[test]
    fn a_named_request_does_not_adopt_a_different_client() {
        let now = Utc::now();
        let rows = vec![lease(9, None, Some("Hiddify/2.0"), "1.2.3.4", 1)];
        assert_eq!(
            merge_lease_candidate(
                &app_device(),
                Some(CORE_SUBSCRIPTION_USER_AGENT),
                "1.2.3.4",
                now,
                &rows,
            ),
            None
        );
    }

    /// Из нескольких подходящих строк берётся самая свежая: это последнее
    /// представление того же устройства.
    #[test]
    fn the_freshest_candidate_wins() {
        let now = Utc::now();
        let rows = vec![
            lease(1, Some("dev-old"), None, "1.2.3.4", 9),
            lease(2, Some("dev-new"), None, "1.2.3.4", 1),
        ];
        let matched =
            merge_lease_candidate(&DeviceIdentity::default(), None, "1.2.3.4", now, &rows)
                .expect("хартбит без User-Agent тоже сливается");
        assert_eq!(matched.id, 2);
    }

    /// Ядро называет себя тем же User-Agent, что записан в панели. Разъедутся
    /// строки — слияние перестанет срабатывать молча, и двойная лиза вернётся.
    #[test]
    fn the_core_user_agent_matches_the_go_constant() {
        let core = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../libs/caramba-core/subscription/subscription.go"
        ))
        .expect("нет исходника клиента подписки ядра");
        assert!(
            core.contains(&format!("= \"{CORE_SUBSCRIPTION_USER_AGENT}\"")),
            "User-Agent ядра разошёлся с константой панели"
        );
    }
}

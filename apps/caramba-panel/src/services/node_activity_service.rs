//! Кто сейчас на узле, сколько узел прокачал и сколько ему ещё можно.
//!
//! Единственный источник «онлайна» в панели — heartbeat узла с полем
//! `active_users` (пишется в `node_user_activity`, см. `awg_service`). Опрос
//! Clash API `:9090` с панели для этого больше НЕ используется: у одного узла
//! порт закрыт хостером снаружи, а у hysteria2 в `/connections` поля с
//! пользователем нет вовсе — то есть тот путь давал «ноль онлайна» там, где
//! люди реально были, и отличить это от «правда никого» было нельзя.
//!
//! Три разных числа, которые легко перепутать (в UI у каждого свой тултип):
//!  * «Сейчас» — у кого прошёл трафик в последнем heartbeat-интервале;
//!  * «Онлайн 15 мин» — кого узел видел за последние 15 минут;
//!  * «Настроено» — сколько подписок привязано к узлу (`subscriptions.node_id`),
//!    это план, а не факт.
//!
//! Отдельно про свежесть: строка `node_user_activity` живёт вечно, её
//! `online` — снимок последнего heartbeat. Умерший узел иначе навсегда остался
//! бы с полным залом народа, поэтому «Сейчас» ВСЕГДА ограничено окном по
//! `last_seen_at`, а не только флагом `online`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;

/// Окно «Сейчас»: heartbeat идёт раз в ~30-60 с, берём три интервала, чтобы
/// один пропущенный запрос не гасил живого пользователя.
pub const NOW_WINDOW_SECS: i32 = 180;

/// Окно «Онлайн 15 мин» — как подписано в админке.
pub const ONLINE_WINDOW_SECS: i32 = 900;

/// Сколько держим снапшоты трафика узлов. 180 дней хватает на окно 30 дней с
/// запасом на разбор инцидентов и не превращает таблицу в архив на годы.
pub const SNAPSHOT_RETENTION_DAYS: i32 = 180;

/// Порог «жёлтого» и «красного» на шкале заполнения узла, проценты.
pub const CAPACITY_WARN_PERCENT: i64 = 80;
pub const CAPACITY_CRITICAL_PERCENT: i64 = 95;

/// Что известно про пользователя, стоящего за тегом узла.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedUser {
    pub user_id: i64,
    pub tg_id: Option<i64>,
    pub username: Option<String>,
    pub full_name: Option<String>,
}

/// Идентичность, закодированная в теге узла.
///
/// Тег приходит ровно в двух видах: `user_{identity}` (sing-box и AWG, см.
/// `services::user_tag`) и голый uuid подписки (сторонние клиенты, легаси).
/// `identity` — НЕ всегда `users.tg_id`: у аккаунта без Telegram это
/// отрицательный суррогат `-users.id` (`config_client_identity`), и спутать
/// половины означает приписать активность чужому пользователю.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagIdentity {
    /// Положительная идентичность — это Telegram id.
    TelegramId(i64),
    /// Отрицательная идентичность — суррогат, это `users.id`.
    UserId(i64),
    /// Uuid подписки (vless_uuid или subscription_uuid).
    SubscriptionUuid(String),
}

/// Единственный разбор тега узла в идентичность. Всё, что не разобралось,
/// остаётся неразрешённым тегом, а не «нулевым пользователем».
pub fn classify_user_tag(tag: &str) -> Option<TagIdentity> {
    let tag = tag.trim();
    if let Some(identity) = crate::services::user_tag::parse_user_tag(tag) {
        return match identity {
            0 => None,
            id if id > 0 => Some(TagIdentity::TelegramId(id)),
            id => Some(TagIdentity::UserId(-id)),
        };
    }
    if is_uuid_like(tag) {
        return Some(TagIdentity::SubscriptionUuid(tag.to_string()));
    }
    None
}

/// Формат uuid проверяется по длине групп: резолв всё равно идёт запросом,
/// а лишняя строка в `= ANY($1)` дешевле, чем пропущенный пользователь.
fn is_uuid_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Подпись пользователя в списке. Тег показывается только если про человека
/// не известно вообще ничего: пустая строка в админке выглядит как баг.
pub fn display_for(
    username: Option<&str>,
    full_name: Option<&str>,
    tg_id: Option<i64>,
    tag: &str,
) -> String {
    if let Some(name) = username.map(str::trim).filter(|v| !v.is_empty()) {
        return format!("@{}", name);
    }
    if let Some(name) = full_name.map(str::trim).filter(|v| !v.is_empty()) {
        return name.to_string();
    }
    if let Some(id) = tg_id {
        return format!("tg:{}", id);
    }
    tag.to_string()
}

/// Человеческая давность для колонки «последний раз». Без длинных тире.
pub fn humanize_since(seconds: i64) -> String {
    if seconds < 60 {
        return "только что".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{} мин назад", minutes);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{} ч назад", hours);
    }
    format!("{} дн назад", hours / 24)
}

/// Процент заполнения узла. Лимит 0 или меньше означает «лимит не задан» —
/// шкалу рисовать не от чего, отдаём 0, а не делим на ноль.
pub fn capacity_percent(used: i64, limit: i32) -> i64 {
    if limit <= 0 {
        return 0;
    }
    let percent = used.saturating_mul(100) / limit as i64;
    percent.clamp(0, 999)
}

/// Цвет шкалы: зелёный / жёлтый / красный. Строка, а не enum, потому что
/// значение уходит прямо в класс CSS шаблона.
pub fn capacity_level(percent: i64, limit: i32) -> &'static str {
    if limit <= 0 {
        return "unknown";
    }
    if percent >= CAPACITY_CRITICAL_PERCENT {
        "critical"
    } else if percent >= CAPACITY_WARN_PERCENT {
        "warn"
    } else {
        "ok"
    }
}

/// Три счётчика узла для строки таблицы Servers.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct NodeCounters {
    /// Трафик за последний heartbeat-интервал.
    pub now: i64,
    /// Уникальные пользователи за 15 минут.
    pub online_15m: i64,
    /// Подписки с `node_id` = этот узел.
    pub configured: i64,
}

/// Заполнение узла относительно лимита.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeCapacity {
    /// Действующий лимит: ручной `max_users_override`, иначе расчётный
    /// `max_users`. 0 означает «не задан».
    pub limit: i32,
    /// Лимит задан руками — в UI это отдельная подпись.
    pub manual: bool,
    pub used: i64,
    pub percent: i64,
    pub level: &'static str,
    /// Узел упёрся в потолок: пора поднимать новый.
    pub needs_new_node: bool,
}

/// Трафик узла по окнам, байты.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct NodeTrafficWindows {
    pub last_24h: i64,
    pub last_30d: i64,
}

/// Строка списка «кто на узле».
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeUserRow {
    pub user_tag: String,
    pub user_id: Option<i64>,
    pub tg_id: Option<i64>,
    pub display: String,
    pub device: Option<String>,
    pub online: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_seen_rel: String,
    pub rx_delta: i64,
    pub tx_delta: i64,
}

/// Где пользователь сейчас — для колонок списка Users и карточки.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UserPresence {
    pub online: bool,
    pub node_id: i64,
    pub node_name: String,
    pub node_flag: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub last_seen_rel: String,
}

/// Какой именно список открывают по клику на число в строке узла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeUserList {
    /// «Сейчас»: онлайн на последнем heartbeat.
    Now,
    /// «Онлайн 15 мин».
    Online15m,
    /// «Настроено»: подписки, привязанные к узлу.
    Configured,
}

impl NodeUserList {
    /// Разбор параметра запроса админки. Неизвестное значение — это «Сейчас»,
    /// чтобы кривая ссылка отдавала список, а не 400.
    pub fn from_query(value: Option<&str>) -> Self {
        match value.unwrap_or("now").trim() {
            "online" | "online15" | "online_15m" => Self::Online15m,
            "configured" | "subs" => Self::Configured,
            _ => Self::Now,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Now => "Сейчас на узле",
            Self::Online15m => "Онлайн за 15 минут",
            Self::Configured => "Настроено на узел",
        }
    }
}

#[derive(Clone)]
pub struct NodeActivityService {
    pool: PgPool,
}

/// Строка активности как её отдаёт БД.
#[derive(sqlx::FromRow)]
struct ActivityRow {
    user_tag: String,
    last_seen_at: DateTime<Utc>,
    online: bool,
    rx_delta: i64,
    tx_delta: i64,
}

impl NodeActivityService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // ------------------------------------------------------------------
    // Резолв тега в пользователя
    // ------------------------------------------------------------------

    /// Пакетный резолв тегов узла в пользователей — ЕДИНСТВЕННАЯ реализация.
    ///
    /// Запросов ровно столько, сколько встретилось видов тега (максимум три),
    /// а не по одному на строку: список онлайна рисуется на каждой перерисовке
    /// таблицы узлов, и N+1 здесь стоил бы сотен запросов в секунду.
    pub async fn resolve_user_tags(
        &self,
        tags: &[String],
    ) -> Result<HashMap<String, ResolvedUser>> {
        let mut by_tg: Vec<i64> = Vec::new();
        let mut by_user: Vec<i64> = Vec::new();
        let mut by_uuid: Vec<String> = Vec::new();
        // Обратный путь: идентичность -> все теги, которые её дали.
        let mut tags_by_tg: HashMap<i64, Vec<String>> = HashMap::new();
        let mut tags_by_user: HashMap<i64, Vec<String>> = HashMap::new();
        let mut tags_by_uuid: HashMap<String, Vec<String>> = HashMap::new();

        for tag in tags {
            match classify_user_tag(tag) {
                Some(TagIdentity::TelegramId(id)) => {
                    by_tg.push(id);
                    tags_by_tg.entry(id).or_default().push(tag.clone());
                }
                Some(TagIdentity::UserId(id)) => {
                    by_user.push(id);
                    tags_by_user.entry(id).or_default().push(tag.clone());
                }
                Some(TagIdentity::SubscriptionUuid(uuid)) => {
                    by_uuid.push(uuid.clone());
                    tags_by_uuid.entry(uuid).or_default().push(tag.clone());
                }
                None => {}
            }
        }

        let mut out: HashMap<String, ResolvedUser> = HashMap::new();

        if !by_tg.is_empty() {
            let rows = sqlx::query_as::<_, (i64, Option<i64>, Option<String>, Option<String>)>(
                "SELECT id, tg_id, username, full_name FROM users WHERE tg_id = ANY($1)",
            )
            .bind(&by_tg)
            .fetch_all(&self.pool)
            .await?;
            for (id, tg_id, username, full_name) in rows {
                let Some(tg) = tg_id else { continue };
                for tag in tags_by_tg.get(&tg).into_iter().flatten() {
                    out.insert(
                        tag.clone(),
                        ResolvedUser {
                            user_id: id,
                            tg_id,
                            username: username.clone(),
                            full_name: full_name.clone(),
                        },
                    );
                }
            }
        }

        if !by_user.is_empty() {
            let rows = sqlx::query_as::<_, (i64, Option<i64>, Option<String>, Option<String>)>(
                "SELECT id, tg_id, username, full_name FROM users WHERE id = ANY($1)",
            )
            .bind(&by_user)
            .fetch_all(&self.pool)
            .await?;
            for (id, tg_id, username, full_name) in rows {
                for tag in tags_by_user.get(&id).into_iter().flatten() {
                    out.insert(
                        tag.clone(),
                        ResolvedUser {
                            user_id: id,
                            tg_id,
                            username: username.clone(),
                            full_name: full_name.clone(),
                        },
                    );
                }
            }
        }

        if !by_uuid.is_empty() {
            let rows = sqlx::query_as::<
                _,
                (
                    Option<String>,
                    Option<String>,
                    i64,
                    Option<i64>,
                    Option<String>,
                    Option<String>,
                ),
            >(
                "SELECT s.vless_uuid, s.subscription_uuid, u.id, u.tg_id, u.username, u.full_name
                 FROM subscriptions s
                 JOIN users u ON u.id = s.user_id
                 WHERE s.vless_uuid = ANY($1) OR s.subscription_uuid = ANY($1)",
            )
            .bind(&by_uuid)
            .fetch_all(&self.pool)
            .await?;
            for (vless, sub_uuid, id, tg_id, username, full_name) in rows {
                for uuid in [vless, sub_uuid].into_iter().flatten() {
                    for tag in tags_by_uuid.get(&uuid).into_iter().flatten() {
                        out.insert(
                            tag.clone(),
                            ResolvedUser {
                                user_id: id,
                                tg_id,
                                username: username.clone(),
                                full_name: full_name.clone(),
                            },
                        );
                    }
                }
            }
        }

        Ok(out)
    }

    // ------------------------------------------------------------------
    // Счётчики узлов
    // ------------------------------------------------------------------

    /// Три счётчика по всем узлам сразу: два запроса на всю таблицу Servers.
    pub async fn counters_by_node(&self) -> Result<HashMap<i64, NodeCounters>> {
        let mut out: HashMap<i64, NodeCounters> = HashMap::new();

        let activity = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT node_id,
                    COUNT(*) FILTER (
                        WHERE online AND last_seen_at > NOW() - ($1::int * INTERVAL '1 second')
                    )::bigint AS now_count,
                    COUNT(DISTINCT user_tag) FILTER (
                        WHERE last_seen_at > NOW() - ($2::int * INTERVAL '1 second')
                    )::bigint AS online_count
             FROM node_user_activity
             GROUP BY node_id",
        )
        .bind(NOW_WINDOW_SECS)
        .bind(ONLINE_WINDOW_SECS)
        .fetch_all(&self.pool)
        .await?;

        for (node_id, now, online_15m) in activity {
            let entry = out.entry(node_id).or_default();
            entry.now = now;
            entry.online_15m = online_15m;
        }

        let configured = sqlx::query_as::<_, (i64, i64)>(
            "SELECT node_id, COUNT(*)::bigint
             FROM subscriptions
             WHERE status = 'active' AND node_id IS NOT NULL
             GROUP BY node_id",
        )
        .fetch_all(&self.pool)
        .await?;

        for (node_id, count) in configured {
            out.entry(node_id).or_default().configured = count;
        }

        Ok(out)
    }

    /// Заполнение каждого узла относительно лимита. «Сейчас» берётся как
    /// нагрузка: настроенные подписки это план, а не люди на проводе.
    pub async fn capacity_by_node(&self) -> Result<HashMap<i64, NodeCapacity>> {
        let counters = self.counters_by_node().await?;

        let limits = sqlx::query_as::<_, (i64, Option<i32>, Option<i32>)>(
            "SELECT id, max_users_override, max_users FROM nodes",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = HashMap::new();
        for (node_id, override_limit, calculated) in limits {
            let manual = override_limit.is_some_and(|v| v > 0);
            let limit = override_limit
                .filter(|v| *v > 0)
                .or(calculated)
                .unwrap_or(0)
                .max(0);
            let used = counters.get(&node_id).map(|c| c.now).unwrap_or(0);
            let percent = capacity_percent(used, limit);
            let level = capacity_level(percent, limit);
            out.insert(
                node_id,
                NodeCapacity {
                    limit,
                    manual,
                    used,
                    percent,
                    level,
                    needs_new_node: level == "critical",
                },
            );
        }

        Ok(out)
    }

    // ------------------------------------------------------------------
    // Списки пользователей узла
    // ------------------------------------------------------------------

    /// Список за числом в строке узла. `limit` ограничивает выдачу: попап не
    /// должен уметь вытащить всю базу.
    pub async fn node_user_list(
        &self,
        node_id: i64,
        kind: NodeUserList,
        limit: i64,
    ) -> Result<Vec<NodeUserRow>> {
        let limit = limit.clamp(1, 100);
        if kind == NodeUserList::Configured {
            return self.configured_list(node_id, limit).await;
        }

        let (window, only_online) = match kind {
            NodeUserList::Now => (NOW_WINDOW_SECS, true),
            _ => (ONLINE_WINDOW_SECS, false),
        };

        let rows = sqlx::query_as::<_, ActivityRow>(
            "SELECT user_tag, last_seen_at, online, rx_delta, tx_delta
             FROM node_user_activity
             WHERE node_id = $1
               AND last_seen_at > NOW() - ($2::int * INTERVAL '1 second')
               AND (NOT $3::bool OR online)
             ORDER BY online DESC, last_seen_at DESC
             LIMIT $4",
        )
        .bind(node_id)
        .bind(window)
        .bind(only_online)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let tags: Vec<String> = rows.iter().map(|r| r.user_tag.clone()).collect();
        let resolved = self.resolve_user_tags(&tags).await?;
        let user_ids: Vec<i64> = resolved.values().map(|r| r.user_id).collect();
        let devices = self.devices_on_node(node_id, &user_ids).await?;

        let now = Utc::now();
        let out = rows
            .into_iter()
            .map(|row| {
                let user = resolved.get(&row.user_tag);
                let display = display_for(
                    user.and_then(|u| u.username.as_deref()),
                    user.and_then(|u| u.full_name.as_deref()),
                    user.and_then(|u| u.tg_id),
                    &row.user_tag,
                );
                let age = (now - row.last_seen_at).num_seconds().max(0);
                NodeUserRow {
                    user_tag: row.user_tag,
                    user_id: user.map(|u| u.user_id),
                    tg_id: user.and_then(|u| u.tg_id),
                    display,
                    device: user.and_then(|u| devices.get(&u.user_id).cloned()),
                    online: row.online,
                    last_seen_at: Some(row.last_seen_at),
                    last_seen_rel: humanize_since(age),
                    rx_delta: row.rx_delta,
                    tx_delta: row.tx_delta,
                }
            })
            .collect();

        Ok(out)
    }

    /// «Настроено»: подписки, привязанные к узлу. Это список из конфигурации,
    /// поэтому ни онлайна, ни дельт трафика у строк нет.
    async fn configured_list(&self, node_id: i64, limit: i64) -> Result<Vec<NodeUserRow>> {
        let rows = sqlx::query_as::<_, (i64, Option<i64>, Option<String>, Option<String>)>(
            "SELECT u.id, u.tg_id, u.username, u.full_name
             FROM subscriptions s
             JOIN users u ON u.id = s.user_id
             WHERE s.node_id = $1 AND s.status = 'active'
             ORDER BY u.username NULLS LAST, u.id
             LIMIT $2",
        )
        .bind(node_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(user_id, tg_id, username, full_name)| {
                let tag = crate::services::user_tag::user_tag(
                    caramba_db::repositories::subscription_repo::config_client_identity(
                        tg_id, user_id,
                    ),
                );
                let display = display_for(username.as_deref(), full_name.as_deref(), tg_id, &tag);
                NodeUserRow {
                    user_tag: tag,
                    user_id: Some(user_id),
                    tg_id,
                    display,
                    device: None,
                    online: false,
                    last_seen_at: None,
                    last_seen_rel: String::new(),
                    rx_delta: 0,
                    tx_delta: 0,
                }
            })
            .collect())
    }

    /// Последнее известное устройство каждого пользователя на этом узле.
    /// Лизы всё ещё висят на подписке, поэтому идём через `subscriptions`.
    async fn devices_on_node(
        &self,
        node_id: i64,
        user_ids: &[i64],
    ) -> Result<HashMap<i64, String>> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, (i64, Option<String>, Option<String>)>(
            "SELECT DISTINCT ON (s.user_id) s.user_id, l.device_name, l.user_agent
             FROM subscription_device_leases l
             JOIN subscriptions s ON s.id = l.subscription_id
             WHERE l.last_node_id = $1 AND s.user_id = ANY($2)
             ORDER BY s.user_id, l.last_seen_at DESC",
        )
        .bind(node_id)
        .bind(user_ids)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|(user_id, name, ua)| {
                let label = name
                    .filter(|v| !v.trim().is_empty())
                    .or_else(|| ua.filter(|v| !v.trim().is_empty()))?;
                Some((user_id, label))
            })
            .collect())
    }

    // ------------------------------------------------------------------
    // Присутствие пользователей (список Users и карточка)
    // ------------------------------------------------------------------

    /// Где пользователи сейчас. Без N+1: одна выборка активности, один пакетный
    /// резолв тегов, независимо от длины страницы Users.
    ///
    /// `user_ids` пустой означает «все» — вызывающий обычно передаёт id строк
    /// текущей страницы.
    pub async fn presence_for_users(&self, user_ids: &[i64]) -> Result<HashMap<i64, UserPresence>> {
        let rows = sqlx::query_as::<_, (String, i64, String, Option<String>, DateTime<Utc>, bool)>(
            "SELECT a.user_tag, n.id, n.name, n.flag, a.last_seen_at, a.online
             FROM node_user_activity a
             JOIN nodes n ON n.id = a.node_id
             WHERE a.last_seen_at > NOW() - ($1::int * INTERVAL '1 second')
             ORDER BY a.online DESC, a.last_seen_at DESC",
        )
        .bind(ONLINE_WINDOW_SECS)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(HashMap::new());
        }

        let tags: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
        let resolved = self.resolve_user_tags(&tags).await?;

        let wanted: Option<std::collections::HashSet<i64>> = if user_ids.is_empty() {
            None
        } else {
            Some(user_ids.iter().copied().collect())
        };

        let now = Utc::now();
        let mut out: HashMap<i64, UserPresence> = HashMap::new();
        // Строки уже отсортированы «сначала онлайн, потом свежие», поэтому
        // первая встреченная запись пользователя и есть лучшая.
        for (tag, node_id, node_name, node_flag, last_seen_at, online) in rows {
            let Some(user) = resolved.get(&tag) else {
                continue;
            };
            if let Some(filter) = &wanted
                && !filter.contains(&user.user_id)
            {
                continue;
            }
            if out.contains_key(&user.user_id) {
                continue;
            }
            let age = (now - last_seen_at).num_seconds().max(0);
            out.insert(
                user.user_id,
                UserPresence {
                    online: online && age <= NOW_WINDOW_SECS as i64,
                    node_id,
                    node_name,
                    node_flag,
                    last_seen_at,
                    last_seen_rel: humanize_since(age),
                },
            );
        }

        Ok(out)
    }

    /// Присутствие одного пользователя — для карточки user_details.
    pub async fn presence_for_user(&self, user_id: i64) -> Result<Option<UserPresence>> {
        Ok(self.presence_for_users(&[user_id]).await?.remove(&user_id))
    }

    // ------------------------------------------------------------------
    // Снапшоты трафика узлов
    // ------------------------------------------------------------------

    /// Замер накопительных счётчиков всех узлов. Зовётся раз в 10 минут из
    /// `TrafficService`.
    pub async fn take_traffic_snapshot(&self) -> Result<u64> {
        let res = sqlx::query(
            "INSERT INTO node_traffic_snapshots (node_id, ts, total_ingress, total_egress)
             SELECT id, NOW(), COALESCE(total_ingress, 0), COALESCE(total_egress, 0)
             FROM nodes",
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Ретеншен снапшотов.
    pub async fn purge_old_snapshots(&self) -> Result<u64> {
        let res = sqlx::query(
            "DELETE FROM node_traffic_snapshots
             WHERE ts < NOW() - ($1::int * INTERVAL '1 day')",
        )
        .bind(SNAPSHOT_RETENTION_DAYS)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Трафик узлов за 24 часа и 30 дней.
    ///
    /// Считается суммой ПОЛОЖИТЕЛЬНЫХ шагов между соседними замерами:
    /// переустановка узла обнуляет счётчики, и «последний минус первый» после
    /// такого рестарта показал бы минус или ноль вместо реального трафика.
    pub async fn traffic_by_node(&self) -> Result<HashMap<i64, NodeTrafficWindows>> {
        let rows = sqlx::query_as::<_, (i64, Option<i64>, Option<i64>)>(
            "SELECT node_id,
                    SUM(step) FILTER (WHERE ts > NOW() - INTERVAL '24 hours')::bigint AS last_24h,
                    SUM(step)::bigint AS last_30d
             FROM (
                 SELECT node_id,
                        ts,
                        GREATEST(total_ingress - LAG(total_ingress)
                            OVER (PARTITION BY node_id ORDER BY ts), 0)
                        + GREATEST(total_egress - LAG(total_egress)
                            OVER (PARTITION BY node_id ORDER BY ts), 0) AS step
                 FROM node_traffic_snapshots
                 WHERE ts > NOW() - INTERVAL '30 days'
             ) steps
             WHERE step IS NOT NULL
             GROUP BY node_id",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(node_id, last_24h, last_30d)| {
                (
                    node_id,
                    NodeTrafficWindows {
                        last_24h: last_24h.unwrap_or(0),
                        last_30d: last_30d.unwrap_or(0),
                    },
                )
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Тег Telegram-пользователя и тег аккаунта без Telegram разбираются в
    /// РАЗНЫЕ ключи. Спутать их означает показать активность одного человека в
    /// карточке другого: суррогат `-42` это `users.id = 42`, а не `tg_id`.
    #[test]
    fn tag_identity_splits_telegram_and_surrogate() {
        assert_eq!(
            classify_user_tag("user_95679857"),
            Some(TagIdentity::TelegramId(95_679_857))
        );
        assert_eq!(classify_user_tag("user_-42"), Some(TagIdentity::UserId(42)));
    }

    /// Uuid подписки — это тоже валидный тег (сторонние клиенты и AWG-легаси).
    #[test]
    fn tag_identity_accepts_subscription_uuid() {
        assert_eq!(
            classify_user_tag("550e8400-e29b-41d4-a716-446655440000"),
            Some(TagIdentity::SubscriptionUuid(
                "550e8400-e29b-41d4-a716-446655440000".to_string()
            ))
        );
    }

    /// Мусор не должен превращаться в пользователя: лучше строка «тег не
    /// разобран», чем чужая активность в карточке.
    #[test]
    fn tag_identity_rejects_garbage() {
        assert_eq!(classify_user_tag("relay_7_legacy"), None);
        assert_eq!(classify_user_tag("user_"), None);
        assert_eq!(classify_user_tag("user_abc"), None);
        assert_eq!(classify_user_tag(""), None);
        // Нулевая идентичность это ошибка генерации, а не пользователь 0.
        assert_eq!(classify_user_tag("user_0"), None);
        // Похоже на uuid по длине, но не hex.
        assert_eq!(
            classify_user_tag("zzzzzzzz-e29b-41d4-a716-446655440000"),
            None
        );
    }

    /// Подпись строки: @username важнее имени, имя важнее tg-id, и только
    /// когда неизвестно ничего — сырой тег.
    #[test]
    fn display_falls_back_in_order() {
        assert_eq!(
            display_for(Some("art"), Some("Art K"), Some(1), "user_1"),
            "@art"
        );
        assert_eq!(display_for(None, Some("Art K"), Some(1), "user_1"), "Art K");
        assert_eq!(display_for(Some("  "), None, Some(7), "user_7"), "tg:7");
        assert_eq!(display_for(None, None, None, "user_7"), "user_7");
    }

    /// Шкала ёмкости: без лимита нечего делить, с лимитом — пороги 80 и 95.
    #[test]
    fn capacity_thresholds_match_ui_colors() {
        assert_eq!(capacity_percent(10, 0), 0);
        assert_eq!(capacity_level(0, 0), "unknown");
        assert_eq!(capacity_percent(40, 100), 40);
        assert_eq!(capacity_level(40, 100), "ok");
        assert_eq!(capacity_level(capacity_percent(80, 100), 100), "warn");
        assert_eq!(capacity_level(capacity_percent(95, 100), 100), "critical");
        // Перебор лимита не ломает вёрстку: процент упирается в потолок.
        assert_eq!(capacity_percent(1_000_000, 1), 999);
    }

    #[test]
    fn humanize_since_is_russian_and_short() {
        assert_eq!(humanize_since(5), "только что");
        assert_eq!(humanize_since(120), "2 мин назад");
        assert_eq!(humanize_since(7_200), "2 ч назад");
        assert_eq!(humanize_since(172_800), "2 дн назад");
    }

    #[test]
    fn list_kind_parsing_never_fails() {
        assert_eq!(NodeUserList::from_query(None), NodeUserList::Now);
        assert_eq!(
            NodeUserList::from_query(Some("online")),
            NodeUserList::Online15m
        );
        assert_eq!(
            NodeUserList::from_query(Some("configured")),
            NodeUserList::Configured
        );
        assert_eq!(NodeUserList::from_query(Some("что-то")), NodeUserList::Now);
    }
}

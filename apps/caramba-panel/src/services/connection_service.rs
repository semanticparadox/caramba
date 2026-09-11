use anyhow::{Context, Result};

use serde::{Deserialize, Serialize};

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::services::orchestration_service::OrchestrationService;
use crate::services::store_service::StoreService;
use crate::services::subscription_service::SubscriptionService;

/// Represents a single connection from the Clash API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClashConnection {
    pub id: String,
    #[serde(rename = "metadata")]
    pub metadata: ConnectionMetadata,
    #[serde(default)]
    pub chains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionMetadata {
    #[serde(default)]
    pub network: String,
    #[serde(rename = "type")]
    pub conn_type: String,
    #[serde(rename = "sourceIP")]
    pub source_ip: String,
    #[serde(rename = "sourcePort")]
    pub source_port: String,
    #[serde(rename = "destinationIP")]
    pub destination_ip: String,
    #[serde(rename = "destinationPort")]
    pub destination_port: String,
    #[serde(default)]
    pub host: String,
    #[serde(rename = "inboundIP", default)]
    pub inbound_ip: String,
    #[serde(rename = "inboundPort", default)]
    pub inbound_port: String,
    #[serde(default)]
    pub user: Option<String>,
}

/// Clash API response for /connections
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClashConnectionsResponse {
    #[serde(default)]
    pub connections: Vec<ClashConnection>,
}

#[derive(Debug, Clone)]
struct NodeConnectionRef {
    node_host: String,
    connection_id: String,
    secret: Option<String>,
}

/// Итоги одного цикла опроса — их копит [`Diag`], а в лог они уходят редко.
#[derive(Debug, Default, Clone)]
struct CycleStats {
    fetched: u64,
    resolved_by_tag: u64,
    resolved_by_uuid: u64,
    unresolved: u64,
    node_errors: u64,
    failing_nodes: std::collections::BTreeSet<String>,
}

impl CycleStats {
    fn merge(&mut self, other: &CycleStats) {
        self.fetched += other.fetched;
        self.resolved_by_tag += other.resolved_by_tag;
        self.resolved_by_uuid += other.resolved_by_uuid;
        self.unresolved += other.unresolved;
        self.node_errors += other.node_errors;
        self.failing_nodes
            .extend(other.failing_nodes.iter().cloned());
    }
}

/// Накопитель между редкими отчётами в лог.
#[derive(Default)]
struct Diag {
    cycles: u32,
    window: CycleStats,
    last_report: Option<std::time::Instant>,
}

/// Как часто сервис вообще подаёт голос. Узел с закрытым снаружи портом 9090
/// (у одного из трёх так и есть — закрыто хостером) давал ERROR каждые пять
/// минут, то есть ~300 строк в сутки об одном и том же известном факте, и
/// топил настоящие ошибки. Один отчёт в час говорит ровно столько же.
const DIAG_REPORT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3600);

/// Аренды устройств best-effort по Clash API узлов.
///
/// Этот сервис БОЛЬШЕ НЕ источник «кто онлайн»: онлайн приходит heartbeat'ом
/// узла (`node_user_activity`, см. `node_activity_service`). Причина не в
/// стиле, а в том, что опрос `:9090` физически не работает как источник:
/// у одного узла порт закрыт снаружи хостером (таймаут TCP, не 401), а у
/// hysteria2 в `/connections` поля с пользователем нет вовсе — то есть даже
/// успешный опрос даёт «никого», что неотличимо от честного нуля.
///
/// Что здесь осталось: подновление аренд устройств там, где опрос проходит, и
/// принудительный разрыв соединений (`kill_subscription_connections`), который
/// зовут квоты и kill-switch. Всё это best-effort: неудача не считается
/// аварией и не должна шуметь в логе.
pub struct ConnectionService {
    orchestration: Arc<OrchestrationService>,
    store: Arc<StoreService>,
    subscription: Arc<SubscriptionService>,
    // Единственный HTTP-клиент — переиспользуется для всех запросов к Clash API
    http_client: reqwest::Client,
    // Счётчики резолва между отчётами. std-мьютекс: под ним нет ни одного
    // await, а tokio-мьютекс потребовал бы async в путях логирования.
    diag: std::sync::Mutex<Diag>,
}

impl ConnectionService {
    pub fn new(
        orchestration: Arc<OrchestrationService>,
        store: Arc<StoreService>,
        subscription: Arc<SubscriptionService>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client for ConnectionService");
        Self {
            orchestration,
            store,
            subscription,
            http_client,
            diag: std::sync::Mutex::new(Diag::default()),
        }
    }

    /// Копит итоги цикла и раз в час пишет ОДНУ строку.
    ///
    /// Строка отвечает на вопрос, ради которого сервис вообще оставлен: на
    /// каком шаге теряются соединения — узел не ответил, тег не разобрался или
    /// подписка не нашлась. Без этих счётчиков резолв был чёрным ящиком.
    fn note_cycle(&self, stats: CycleStats) {
        let Ok(mut diag) = self.diag.lock() else {
            return;
        };
        diag.cycles += 1;
        let merged = stats;
        diag.window.merge(&merged);

        let due = diag
            .last_report
            .map(|t| t.elapsed() >= DIAG_REPORT_INTERVAL)
            .unwrap_or(true);
        if !due {
            return;
        }

        diag.last_report = Some(std::time::Instant::now());
        let cycles = std::mem::take(&mut diag.cycles);
        let window = std::mem::take(&mut diag.window);
        drop(diag);

        let failing = if window.failing_nodes.is_empty() {
            "нет".to_string()
        } else {
            window
                .failing_nodes
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        };
        info!(
            "Аренды устройств (best-effort) за {} циклов: соединений {}, по тегу {}, по uuid {}, не разобрано {}, отказов опроса {} (узлы: {})",
            cycles,
            window.fetched,
            window.resolved_by_tag,
            window.resolved_by_uuid,
            window.unresolved,
            window.node_errors,
            failing
        );
    }

    /// Start the background monitoring loop
    pub async fn start_monitoring(&self) {
        info!(
            "Аренды устройств: фоновый опрос узлов раз в 5 минут, отчёт в лог раз в час (онлайн считается не здесь, а по heartbeat)"
        );
        let mut interval = time::interval(time::Duration::from_secs(300)); // 5 minutes

        loop {
            interval.tick().await;

            // Провал цикла — не авария: это best-effort путь, единственное
            // его последствие — аренды устройств не обновились в этот раз.
            if let Err(e) = self.check_and_enforce_limits().await {
                warn!("Цикл аренд устройств не прошёл: {:#}", e);
            }

            // Cleanup old IP tracking records (>1 hour old)
            if let Err(e) = self.subscription.cleanup_old_ip_tracking().await {
                warn!("Не удалось почистить старые записи IP: {:#}", e);
            }
        }
    }

    /// Main enforcement logic: check all nodes and enforce device limits
    async fn check_and_enforce_limits(&self) -> Result<()> {
        let mut stats = CycleStats::default();

        // Get all active nodes
        let nodes: Vec<caramba_db::models::node::Node> =
            self.orchestration.node_repo.get_all_nodes().await?;
        let infra_ips: HashSet<IpAddr> =
            nodes.iter().filter_map(|n| parse_ip_maybe(&n.ip)).collect();

        if nodes.is_empty() {
            warn!("No active nodes found, skipping device limit check");
            return Ok(());
        }

        // Collect connections from all nodes
        // Key: Subscription ID (i64), Value: Set of IPs
        let mut subscription_ips: HashMap<i64, HashSet<String>> = HashMap::new();
        let mut subscription_connections: HashMap<i64, HashMap<String, Vec<NodeConnectionRef>>> =
            HashMap::new();

        // Separate cache for resolving UUID -> SubID to avoid repeatedly DB hitting if using chains
        let mut uuid_cache: HashMap<String, i64> = HashMap::new();
        // Cache for resolving tg_id -> active subscription id (Strategy 1).
        // None is cached too: a tag whose user has no active subscription
        // shouldn't be re-queried for every connection in the cycle.
        let mut tg_id_cache: HashMap<i64, Option<i64>> = HashMap::new();

        for node in nodes {
            if node.status != "active" {
                continue;
            }
            match self
                .fetch_node_connections(&node.ip, node.clash_api_secret.as_deref())
                .await
            {
                Ok(connections) => {
                    stats.fetched += connections.len() as u64;
                    debug!(
                        "Fetched {} connections from node {}",
                        connections.len(),
                        node.ip
                    );

                    for conn in connections {
                        // Strategy 1: metadata.user carries "user_{tg_id}" —
                        // the Telegram id, NOT a subscription id (sing-box
                        // configs are generated that way, see
                        // services::user_tag). Resolve tg_id -> user ->
                        // active subscription, mirroring the traffic
                        // accounting path in api/v2/node.rs::heartbeat.
                        let mut sub_id_opt = None;

                        if let Some(user_tag) = &conn.metadata.user
                            && let Some(tg_id) = crate::services::user_tag::parse_user_tag(user_tag)
                        {
                            sub_id_opt = match tg_id_cache.get(&tg_id) {
                                Some(cached) => *cached,
                                None => {
                                    let resolved = match self
                                        .subscription
                                        .get_active_subscription_id_by_tg_id(tg_id)
                                        .await
                                    {
                                        Ok(id) => id,
                                        Err(e) => {
                                            warn!(
                                                "Failed to resolve tg_id {} to a subscription: {:#}",
                                                tg_id, e
                                            );
                                            None
                                        }
                                    };
                                    tg_id_cache.insert(tg_id, resolved);
                                    resolved
                                }
                            };
                        }

                        // Счётчики по шагам резолва: без них не видно, где
                        // именно теряются соединения на живом узле.
                        let resolved_by_tag = sub_id_opt.is_some();
                        if resolved_by_tag {
                            stats.resolved_by_tag += 1;
                        }

                        // Strategy 2: Check chains for UUID if Strategy 1 failed
                        if sub_id_opt.is_none()
                            && let Some(uuid) = extract_uuid_from_chain(&conn)
                        {
                            if let Some(cached_id) = uuid_cache.get(&uuid) {
                                sub_id_opt = Some(*cached_id);
                            } else {
                                // Resolve UUID to ID from DB
                                if let Ok(Some(sub)) =
                                    self.store.get_subscription_by_uuid(&uuid).await
                                {
                                    uuid_cache.insert(uuid.clone(), sub.id);
                                    sub_id_opt = Some(sub.id);
                                }
                            }
                        }

                        if !resolved_by_tag && sub_id_opt.is_some() {
                            stats.resolved_by_uuid += 1;
                        }
                        if sub_id_opt.is_none() {
                            stats.unresolved += 1;
                        }

                        if let Some(sub_id) = sub_id_opt {
                            let Some(normalized_ip) = normalize_client_ip(&conn.metadata.source_ip)
                            else {
                                continue;
                            };
                            if should_skip_source_ip(&normalized_ip, &infra_ips) {
                                continue;
                            }
                            subscription_ips
                                .entry(sub_id)
                                .or_default()
                                .insert(normalized_ip.clone());
                            subscription_connections
                                .entry(sub_id)
                                .or_default()
                                .entry(normalized_ip)
                                .or_default()
                                .push(NodeConnectionRef {
                                    node_host: node.ip.clone(),
                                    connection_id: conn.id.clone(),
                                    secret: node.clash_api_secret.clone(),
                                });
                        }
                    }
                }
                Err(e) => {
                    // Одна и та же недоступность повторяется каждые пять
                    // минут: подробность в debug, факт — в часовой отчёт.
                    stats.node_errors += 1;
                    stats.failing_nodes.insert(node.ip.clone());
                    debug!("Узел {} не отдал соединения: {:#}", node.ip, e);
                    continue;
                }
            }
        }

        debug!("Collected IPs for {} subscriptions", subscription_ips.len());
        self.note_cycle(stats);

        // Check each subscription's device limit
        for (sub_id, ips) in subscription_ips {
            if let Err(e) = self
                .enforce_subscription_limit(sub_id, ips, subscription_connections.get(&sub_id))
                .await
            {
                error!(
                    "Failed to enforce limit for subscription ID {}: {:#}",
                    sub_id, e
                );
            }
        }

        Ok(())
    }

    /// Fetch active connections from a node's Clash API
    async fn fetch_node_connections(
        &self,
        node_host: &str,
        secret: Option<&str>,
    ) -> Result<Vec<ClashConnection>> {
        let url = format!("http://{}:9090/connections", node_host);

        let mut request = self.http_client.get(&url);
        if let Some(token) = secret.filter(|s| !s.is_empty()) {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to fetch connections from {}", node_host))?;

        if !response.status().is_success() {
            anyhow::bail!("Clash API returned error status: {}", response.status());
        }

        let data: ClashConnectionsResponse = response
            .json()
            .await
            .context("Failed to parse Clash API response")?;

        Ok(data.connections)
    }

    /// Enforce device limit for a single subscription
    async fn enforce_subscription_limit(
        &self,
        sub_id: i64,
        active_ips: HashSet<String>,
        active_connections: Option<&HashMap<String, Vec<NodeConnectionRef>>>,
    ) -> Result<()> {
        // Get device limit for this subscription
        let device_limit = self
            .subscription
            .get_subscription_device_limit(sub_id)
            .await?;

        let active_device_count = active_ips.len();
        let ips_vec: Vec<String> = active_ips.iter().cloned().collect();

        // Update live tracking in database (legacy IP tracking + device lease heartbeat).
        self.subscription.update_ips(sub_id, ips_vec).await?;

        // Check if limit exceeded (0 for Unlimited)
        if device_limit > 0 && active_device_count > device_limit as usize {
            warn!(
                "Subscription {} exceeded device limit: {}/{} devices. Enforcing limit.",
                sub_id, active_device_count, device_limit
            );

            let blocked_ips = self
                .select_blocked_ips(sub_id, &active_ips, device_limit as usize)
                .await?;

            if blocked_ips.is_empty() {
                // Safety fallback if ordering logic couldn't pick victims.
                self.kill_subscription_connections(sub_id).await?;
                return Ok(());
            }

            self.kill_subscription_connections_for_ips(sub_id, &blocked_ips, active_connections)
                .await?;

            let blocked_vec: Vec<String> = blocked_ips.into_iter().collect();
            let _ = self
                .subscription
                .remove_tracked_ips(sub_id, &blocked_vec)
                .await
                .map_err(|e| {
                    warn!(
                        "Failed to purge blocked device IP records for sub {}: {}",
                        sub_id, e
                    );
                    e
                });
        } else if active_device_count > 0 {
            debug!(
                "Subscription {} within limit: {}/{} devices",
                sub_id,
                active_device_count,
                if device_limit == 0 {
                    "Unlimited".to_string()
                } else {
                    device_limit.to_string()
                }
            );
        }

        Ok(())
    }

    async fn select_blocked_ips(
        &self,
        sub_id: i64,
        active_ips: &HashSet<String>,
        device_limit: usize,
    ) -> Result<HashSet<String>> {
        if device_limit == 0 || active_ips.len() <= device_limit {
            return Ok(HashSet::new());
        }

        let tracked_ips = self
            .subscription
            .get_active_ips(sub_id)
            .await
            .unwrap_or_default();
        let mut ordered: Vec<String> = tracked_ips
            .into_iter()
            .map(|row| row.client_ip)
            .filter(|ip| active_ips.contains(ip))
            .collect();

        ordered.sort();
        ordered.dedup();

        let mut remaining: Vec<String> = active_ips
            .iter()
            .filter(|ip| !ordered.contains(ip))
            .cloned()
            .collect();
        remaining.sort();
        ordered.extend(remaining);

        let preferred_ip_raw: Option<String> = sqlx::query_scalar::<_, Option<String>>(
            "SELECT last_access_ip FROM subscriptions WHERE id = $1",
        )
        .bind(sub_id)
        .fetch_optional(&self.orchestration.pool)
        .await?
        .flatten();
        let preferred_ip = preferred_ip_raw.and_then(|ip| normalize_client_ip(&ip));

        if let Some(preferred) = preferred_ip
            && let Some(pos) = ordered.iter().position(|ip| ip == &preferred)
        {
            let value = ordered.remove(pos);
            ordered.insert(0, value);
        }

        let blocked: HashSet<String> = ordered.into_iter().skip(device_limit).collect();
        Ok(blocked)
    }

    async fn kill_subscription_connections_for_ips(
        &self,
        sub_id: i64,
        blocked_ips: &HashSet<String>,
        active_connections: Option<&HashMap<String, Vec<NodeConnectionRef>>>,
    ) -> Result<()> {
        if blocked_ips.is_empty() {
            return Ok(());
        }

        let mut killed = 0usize;
        if let Some(by_ip) = active_connections {
            for ip in blocked_ips {
                if let Some(connections) = by_ip.get(ip) {
                    for conn in connections {
                        match self
                            .close_connection(
                                &conn.node_host,
                                &conn.connection_id,
                                conn.secret.as_deref(),
                            )
                            .await
                        {
                            Ok(_) => killed += 1,
                            Err(e) => {
                                error!(
                                    "Failed to close overflow connection {} on {} for sub {} (ip {}): {}",
                                    conn.connection_id, conn.node_host, sub_id, ip, e
                                );
                            }
                        }
                    }
                }
            }
        }

        if killed == 0 {
            warn!(
                "No concrete overflow connection IDs found for sub {}. Falling back to kill-all.",
                sub_id
            );
            self.kill_subscription_connections(sub_id).await?;
        } else {
            info!(
                "Killed {} overflow connections for sub {} ({} blocked IPs)",
                killed,
                sub_id,
                blocked_ips.len()
            );
        }

        Ok(())
    }

    /// Kill all active connections for a specific subscription across all nodes
    pub async fn kill_subscription_connections(&self, sub_id: i64) -> Result<()> {
        // sing-box tags connections "user_{tg_id}" (Telegram id), never
        // "user_{sub_id}" — resolve the subscription's owner first, otherwise
        // the target tag matches nothing and enforcement is a silent no-op.
        let Some((tg_id, vless_uuid)) = self
            .subscription
            .get_subscription_connection_identity(sub_id)
            .await?
        else {
            warn!(
                "Cannot kill connections for subscription {}: no such subscription",
                sub_id
            );
            return Ok(());
        };

        // The per-user tag cannot distinguish WHICH subscription a connection
        // belongs to. If the user still has another active subscription (e.g.
        // a paid plan alongside a throttled free one), a tag-based kill would
        // also drop the sessions that other subscription legitimately serves —
        // so in that case only match by the vless UUID specific to this
        // subscription and skip the tag.
        let match_by_tag = match self
            .subscription
            .user_has_other_active_subscription(sub_id)
            .await
        {
            Ok(has_other) => {
                if has_other {
                    info!(
                        "Sub {} owner has another active subscription — skipping tag-based kill, matching by vless UUID only",
                        sub_id
                    );
                }
                !has_other
            }
            // On lookup failure err on the side of enforcement (old behavior).
            Err(e) => {
                warn!(
                    "Could not check other active subscriptions for sub {}: {} — falling back to tag-based kill",
                    sub_id, e
                );
                true
            }
        };

        let nodes: Vec<caramba_db::models::node::Node> =
            self.orchestration.node_repo.get_all_nodes().await?;
        let target_user = crate::services::user_tag::user_tag(tg_id);

        for node in nodes {
            match self
                .fetch_node_connections(&node.ip, node.clash_api_secret.as_deref())
                .await
            {
                Ok(connections) => {
                    for conn in connections {
                        // Check metadata.user against the tg_id tag
                        let mut match_found = false;
                        if match_by_tag
                            && let Some(user) = &conn.metadata.user
                            && user == &target_user
                        {
                            match_found = true;
                        }

                        // Legacy fallback: connections whose chains carry the
                        // subscription's vless UUID.
                        if !match_found
                            && let Some(uuid) = &vless_uuid
                            && conn.chains.iter().any(|chain| chain == uuid)
                        {
                            match_found = true;
                        }

                        if match_found {
                            info!(
                                "Killing connection {} on node {} for {}",
                                conn.id, node.name, target_user
                            );
                            if let Err(e) = self
                                .close_connection(
                                    &node.ip,
                                    &conn.id,
                                    node.clash_api_secret.as_deref(),
                                )
                                .await
                            {
                                error!(
                                    "Failed to close connection {} on {}: {}",
                                    conn.id, node.name, e
                                );
                            }
                        }
                    }
                }
                Err(e) => error!(
                    "Failed to fetch connections from {} during kill: {}",
                    node.name, e
                ),
            }
        }
        Ok(())
    }

    /// Close a specific connection on a node via Clash API
    async fn close_connection(
        &self,
        node_host: &str,
        connection_id: &str,
        secret: Option<&str>,
    ) -> Result<()> {
        let url = format!("http://{}:9090/connections/{}", node_host, connection_id);

        let mut request = self.http_client.delete(&url);
        if let Some(token) = secret.filter(|s| !s.is_empty()) {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to delete connection on {}", node_host))?;

        if !response.status().is_success() {
            // 404 means already gone, which is fine
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(());
            }
            anyhow::bail!("Clash API delete error: {}", response.status());
        }
        Ok(())
    }
}

/// Extract UUID from connection chains (legacy support)
fn extract_uuid_from_chain(conn: &ClashConnection) -> Option<String> {
    for chain in &conn.chains {
        if is_valid_uuid(chain) {
            return Some(chain.clone());
        }
    }
    None
}

fn parse_ip_maybe(value: &str) -> Option<IpAddr> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Ok(ip) = value.parse::<IpAddr>() {
        return Some(canonicalize_ip(ip));
    }

    if let Ok(sock) = value.parse::<std::net::SocketAddr>() {
        return Some(canonicalize_ip(sock.ip()));
    }

    if let Some((host, _port)) = value.rsplit_once(':')
        && let Ok(ip) = host.parse::<IpAddr>()
    {
        return Some(canonicalize_ip(ip));
    }

    None
}

fn canonicalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

fn normalize_client_ip(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "0.0.0.0" || trimmed == "::" {
        return None;
    }
    parse_ip_maybe(trimmed).map(|ip| ip.to_string())
}

fn should_skip_source_ip(source_ip: &str, infra_ips: &HashSet<IpAddr>) -> bool {
    let source = source_ip.trim();

    if source.is_empty() || source == "0.0.0.0" || source == "::" {
        return true;
    }

    if let Some(ip) = parse_ip_maybe(source) {
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
            return true;
        }
        if infra_ips.contains(&ip) {
            return true;
        }
    }

    false
}

/// Simple UUID validation (format check only)
fn is_valid_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }

    parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
}

#[cfg(test)]
mod tests {
    use super::{parse_ip_maybe, should_skip_source_ip};
    use std::collections::HashSet;
    use std::net::IpAddr;
    use std::str::FromStr;

    #[test]
    fn parse_ip_maybe_supports_host_port() {
        assert_eq!(
            parse_ip_maybe("137.74.119.200:443"),
            Some(IpAddr::from_str("137.74.119.200").unwrap())
        );
    }

    #[test]
    fn parse_ip_maybe_normalizes_ipv4_mapped_ipv6() {
        assert_eq!(
            parse_ip_maybe("::ffff:137.74.119.200"),
            Some(IpAddr::from_str("137.74.119.200").unwrap())
        );
    }

    #[test]
    fn should_skip_node_and_loopback_ips() {
        let mut infra = HashSet::new();
        infra.insert(IpAddr::from_str("137.74.119.200").unwrap());

        assert!(should_skip_source_ip("137.74.119.200", &infra));
        assert!(should_skip_source_ip("127.0.0.1", &infra));
        assert!(!should_skip_source_ip("100.6.144.142", &infra));
    }
}

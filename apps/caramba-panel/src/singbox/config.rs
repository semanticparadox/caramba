use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]

pub struct SingBoxConfig {
    pub log: LogConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns: Option<DnsConfig>,
    pub inbounds: Vec<Inbound>,
    pub outbounds: Vec<Outbound>,
    pub route: Option<RouteConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<ExperimentalConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuleSet {
    Remote(RemoteRuleSet),
    Local(LocalRuleSet),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RemoteRuleSet {
    pub tag: String,
    pub format: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_detour: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_interval: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalRuleSet {
    pub tag: String,
    pub format: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExperimentalConfig {
    pub clash_api: ClashApiConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_file: Option<CacheFileConfig>,
    /// Счётчики трафика по пользователям.
    ///
    /// Единственный способ узнать, сколько прокачал конкретный человек:
    /// Clash API таких данных не отдаёт вовсе — в его метаданных соединения
    /// есть network, type, sourceIP, destinationIP, порты, host, dnsMode и
    /// processPath, и ни одного поля с именем пользователя
    /// (experimental/clashapi/trafficontrol/tracker.go, sing-box 1.13).
    ///
    /// Требует сборки sing-box с `-tags with_v2ray_api`: официальный пакет
    /// SagerNet собран без него и отвечает на такой конфиг отказом при старте.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v2ray_api: Option<V2RayApiConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct V2RayApiConfig {
    /// gRPC-слушатель. Только петля: наружу его отдавать незачем, счётчики
    /// снимает агент, живущий на том же узле.
    pub listen: String,
    pub stats: V2RayStatsConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct V2RayStatsConfig {
    pub enabled: bool,
    /// Имена пользователей, по которым вести счёт. sing-box заводит счётчики
    /// `user>>><имя>>>>traffic>>>uplink` и `…>>>downlink` только для
    /// перечисленных здесь (experimental/v2rayapi/stats.go).
    pub users: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CacheFileConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Persist address-filter / rejected DNS-rule results across restarts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_rdrc: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClashApiConfig {
    pub external_controller: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ui: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_control_allow_origin: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_control_allow_private_network: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogConfig {
    pub level: String,
    pub timestamp: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Inbound {
    Vless(VlessInbound),
    Hysteria2(Hysteria2Inbound),
    #[serde(rename = "wireguard")]
    AmneziaWg(AmneziaWgInbound),
    Trojan(TrojanInbound),
    Tuic(TuicInbound),
    Http(HttpInbound),
    Naive(NaiveInbound),
    Shadowsocks(ShadowsocksInbound),
    Shadowtls(ShadowtlsInbound),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<HttpUser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<VlessTlsConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpUser {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NaiveInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<NaiveUser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<VlessTlsConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NaiveUser {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<VlessUser>,
    pub tls: Option<VlessTlsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<VlessTransportConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet_encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiplex: Option<MultiplexConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowsocksInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<ShadowsocksUser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiplex: Option<MultiplexConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowsocksUser {
    pub name: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowtlsInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub version: Option<i32>,
    pub users: Vec<ShadowtlsUser>,
    pub handshake: ShadowtlsHandshake,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detour: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowtlsUser {
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowtlsHandshake {
    pub server: String,
    pub server_port: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessUser {
    pub name: String,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VlessTransportConfig {
    Ws(WsTransport),
    HttpUpgrade(HttpUpgradeTransport),
    Grpc(GrpcTransport),
    #[serde(rename = "xhttp")]
    Xhttp(XhttpTransport),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GrpcTransport {
    pub service_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct XhttpTransport {
    pub path: String,
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WsTransport {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpUpgradeTransport {
    pub path: String,
    /// host — строка, не массив. sing-box: "httpupgrade" transport spec.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VlessTlsConfig {
    pub enabled: bool,
    pub server_name: String,
    // ALPN often needed for Vision/Reality
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
    // Reality блок включается только когда reality.enabled == true.
    // Пустой блок с private_key: "" вызывает FATAL в sing-box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reality: Option<RealityConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RealityConfig {
    pub enabled: bool,
    pub handshake: RealityHandshake,
    pub private_key: String,
    pub short_id: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RealityHandshake {
    pub server: String,
    pub server_port: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2Inbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<Hysteria2User>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_mbps: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub down_mbps: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_client_bandwidth: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfs: Option<Hysteria2Obfs>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub masquerade: Option<String>,

    pub tls: Hysteria2TlsConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2Obfs {
    #[serde(rename = "type")]
    pub ttype: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2User {
    pub name: Option<String>,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hysteria2TlsConfig {
    pub enabled: bool,
    pub server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AmneziaWgInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub peers: Vec<AmneziaWgUser>,
    pub private_key: String,
    // AmneziaWG specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jc: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jmin: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jmax: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s1: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s2: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h2: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h3: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h4: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AmneziaWgUser {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preshared_key: Option<String>,
    pub allowed_ips: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outbound {
    Direct { tag: String },
    Shadowsocks(ShadowsocksOutbound),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShadowsocksOutbound {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub method: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrojanInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<TrojanUser>,
    pub tls: Option<VlessTlsConfig>, // Can reuse VlessTlsConfig or define TrojanTlsConfig
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiplex: Option<MultiplexConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrojanUser {
    pub name: Option<String>,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TuicInbound {
    pub tag: String,
    pub listen: String,
    pub listen_port: u16,
    pub users: Vec<TuicUser>,
    pub congestion_control: String,
    // sing-box parses these as Go durations; an empty/invalid string is FATAL on
    // `sing-box check`. Emit the field only when a non-empty value is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_timeout: Option<String>,
    pub zero_rtt_handshake: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat: Option<String>,
    pub tls: TuicTlsConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TuicUser {
    pub name: Option<String>,
    pub uuid: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TuicTlsConfig {
    pub enabled: bool,
    pub server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteConfig {
    pub rules: Vec<RouteRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_set: Option<Vec<RuleSet>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_domain_resolver: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outbound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<Vec<u16>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geosite: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geoip: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_resolver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_set: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DnsConfig {
    pub servers: Vec<DnsServer>,
    pub rules: Vec<DnsRule>,
    /// Global answer strategy (prefer_ipv4 | prefer_ipv6 | ipv4_only | ipv6_only).
    /// Omitted by default so the legacy output stays byte-identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DnsServer {
    Udp(UdpDnsServer),
    Local(LocalDnsServer),
    /// DNS-over-HTTPS (sing-box 1.13 `type: "https"`).
    Https(HttpsDnsServer),
    /// DNS-over-TLS (sing-box 1.13 `type: "tls"`).
    Tls(TlsDnsServer),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UdpDnsServer {
    pub tag: String,
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detour: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalDnsServer {
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detour: Option<String>,
}

/// DNS-over-HTTPS server (sing-box 1.13 schema, `dns/server/https`).
/// `server` should be an IP literal to avoid a bootstrap resolution loop.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpsDnsServer {
    pub tag: String,
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detour: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_resolver: Option<String>,
}

/// DNS-over-TLS server (sing-box 1.13 schema, `dns/server/tls`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TlsDnsServer {
    pub tag: String,
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detour: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_resolver: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DnsRule {
    /// DNS rule action (sing-box 1.11+). "route" forwards to `server`,
    /// "reject" sinkholes the query. Required form for forward-compat
    /// (the bare `server` field is deprecated and removed in 1.14).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Reject method: "default" (REFUSED) or "drop". Only with action="reject".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_resolver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_set: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MultiplexConfig {
    pub enabled: bool,
    #[serde(default = "default_mux_protocol_singbox")]
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_streams: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<bool>,
}

fn default_mux_protocol_singbox() -> String {
    "smux".to_string()
}

/// Имена пользователей из уже построенных инбаундов.
///
/// Список для `v2ray_api.stats.users` собирается ИЗ КОНФИГА, а не из исходных
/// данных: так он не может разойтись с тем, что реально попало в инбаунды.
/// Разойдись они — счётчик молча не завёлся бы для части людей, и это было бы
/// видно только по нулям в их трафике.
///
/// Порядок стабилен, дубликаты убраны: один человек обычно заведён сразу в
/// нескольких протоколах под одним и тем же именем.
pub fn stats_user_names(inbounds: &[Inbound]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for inbound in inbounds {
        match inbound {
            Inbound::Vless(i) => seen.extend(i.users.iter().map(|u| u.name.clone())),
            Inbound::Shadowsocks(i) => seen.extend(i.users.iter().map(|u| u.name.clone())),
            Inbound::Hysteria2(i) => seen.extend(i.users.iter().filter_map(|u| u.name.clone())),
            Inbound::Trojan(i) => seen.extend(i.users.iter().filter_map(|u| u.name.clone())),
            Inbound::Tuic(i) => seen.extend(i.users.iter().filter_map(|u| u.name.clone())),
            Inbound::Shadowtls(i) => seen.extend(i.users.iter().filter_map(|u| u.name.clone())),
            // http/naive называют пользователя username, а wireguard вообще не
            // имеет пользователей в этом смысле — считать по ним нечего.
            Inbound::Http(_) | Inbound::Naive(_) | Inbound::AmneziaWg(_) => {}
        }
    }
    seen.retain(|name| !name.trim().is_empty());
    seen.into_iter().collect()
}

#[cfg(test)]
mod stats_user_tests {
    use super::*;

    fn vless(users: &[&str]) -> Inbound {
        Inbound::Vless(VlessInbound {
            tag: "vless-in".into(),
            listen: "::".into(),
            listen_port: 443,
            users: users
                .iter()
                .map(|n| VlessUser {
                    name: (*n).to_string(),
                    uuid: "00000000-0000-0000-0000-000000000000".into(),
                    flow: None,
                })
                .collect(),
            tls: None,
            transport: None,
            multiplex: None,
            packet_encoding: None,
        })
    }

    /// Один человек заведён сразу в нескольких протоколах под одним именем —
    /// счётчик ему нужен один, иначе sing-box получит дубликаты в stats.users.
    #[test]
    fn duplicates_across_inbounds_collapse() {
        let names = stats_user_names(&[vless(&["user_1", "user_2"]), vless(&["user_2", "user_3"])]);
        assert_eq!(names, vec!["user_1", "user_2", "user_3"]);
    }

    /// Порядок обязан быть устойчивым: конфиг перегенерируется на каждое
    /// изменение узла, и «тот же список в другом порядке» — это лишняя
    /// перезапись файла и лишний рестарт sing-box.
    #[test]
    fn order_is_stable_regardless_of_input_order() {
        let a = stats_user_names(&[vless(&["user_9", "user_1"])]);
        let b = stats_user_names(&[vless(&["user_1", "user_9"])]);
        assert_eq!(a, b);
    }

    /// Пустые имена в конфиг попадать не должны: sing-box заведёт счётчик с
    /// пустым ключом, а привязать его будет не к кому.
    #[test]
    fn blank_names_are_dropped() {
        assert!(stats_user_names(&[vless(&["", "   "])]).is_empty());
    }
}

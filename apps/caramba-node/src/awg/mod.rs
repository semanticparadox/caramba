//! AmneziaWG на узле: отдельный процесс `amneziawg-go` и интерфейс `awg0`.
//!
//! Зачем отдельным процессом, а не внутри sing-box: официальный sing-box не
//! умеет wireguard-инбаунд с AWG-полями (апстрим-PR закрыт), а собственный форк
//! пришлось бы пересобирать и сопровождать поверх уже существующего
//! самообновления узла. Поэтому AWG живёт своей жизнью: свой бинарь, свой
//! интерфейс, своё управление через UAPI-сокет. sing-box при изменении пиров
//! НЕ перезапускается — это главное требование к модулю, и оно обеспечено уже
//! на уровне контракта: `ConfigResponse.hash` считается только от
//! sing-box-конфига, а раздел `awg` едет со своим `awg_hash`.
//!
//! Источник истины — панель. Узел не генерирует ни ключей, ни адресов, ни
//! параметров обфускации: он приводит интерфейс ровно к тому составу пиров,
//! который прислали.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

pub mod download;
pub mod iface;
pub mod stats;
pub mod uapi;

#[cfg(test)]
pub(crate) use caramba_shared::config::AwgPeer;
/// Контракт раздела живёт в caramba-shared: его пишет панель, читает узел, и
/// двух описаний одной и той же структуры быть не должно.
pub use caramba_shared::config::AwgSection;

/// Имя интерфейса. Одно на узел: несколько AWG-инстансов панель не выдаёт.
pub const AWG_INTERFACE: &str = "awg0";

/// Окно «онлайна» по свежести хендшейка (секунды).
///
/// WireGuard шлёт хендшейк раз в ~120 с при активном трафике, поэтому 180 с —
/// минимальное окно, при котором живой пир не мигает. Значение зафиксировано
/// контрактом heartbeat (п.4 задания), менять его в одиночку нельзя: панель
/// строит на нём «кто онлайн на ноде».
pub const ONLINE_WINDOW_SECS: u64 = 180;

/// MTU интерфейса. 1420 — стандарт WireGuard (1500 − 80 байт оверхеда);
/// AWG добавляет джанк только к хендшейкам, на транспортный MTU он не влияет.
pub const AWG_MTU: u32 = 1420;

/// Проверка раздела ДО применения.
///
/// Fail-closed: кривой раздел не применяется вовсе. Поднять интерфейс с
/// половиной пиров или с невалидными H-параметрами хуже, чем не поднимать:
/// клиенты получат конфиг с ключами, которых сервер не знает, и будут молча
/// стучаться в никуда, а мы будем видеть «AWG работает».
pub fn validate(section: &AwgSection) -> anyhow::Result<()> {
    if section.listen_port == 0 {
        anyhow::bail!("listen_port не задан");
    }
    uapi::key_base64_to_hex(&section.private_key)
        .map_err(|e| anyhow::anyhow!("private_key: {e}"))?;
    parse_cidr(&section.address_cidr)
        .ok_or_else(|| anyhow::anyhow!("address_cidr '{}' не разобран", section.address_cidr))?;

    // H1..H4 заменяют штатные типы пакетов WireGuard 1..4. Совпадение с ними
    // или друг с другом ломает разбор на обеих сторонах: amneziawg-go такой
    // конфиг не примет, и незачем давать ему шанс оставить интерфейс
    // полуживым.
    let h = [section.h1, section.h2, section.h3, section.h4];
    for (i, v) in h.iter().enumerate() {
        if *v <= 4 {
            anyhow::bail!(
                "h{} = {} — зарезервировано под штатные типы WireGuard",
                i + 1,
                v
            );
        }
    }
    for i in 0..h.len() {
        for j in (i + 1)..h.len() {
            if h[i] == h[j] {
                anyhow::bail!("h{} и h{} совпадают ({})", i + 1, j + 1, h[i]);
            }
        }
    }
    if section.jmin > section.jmax {
        anyhow::bail!("jmin {} больше jmax {}", section.jmin, section.jmax);
    }

    let mut seen_keys = std::collections::HashSet::new();
    for peer in &section.peers {
        uapi::key_base64_to_hex(&peer.public_key)
            .map_err(|e| anyhow::anyhow!("peer {}: public_key: {e}", peer.user_tag))?;
        if parse_cidr(&peer.allowed_ip).is_none() {
            anyhow::bail!(
                "peer {}: allowed_ip '{}' не разобран",
                peer.user_tag,
                peer.allowed_ip
            );
        }
        if peer.user_tag.trim().is_empty() {
            anyhow::bail!("peer с ключом {} без user_tag", short_key(&peer.public_key));
        }
        if !seen_keys.insert(peer.public_key.trim().to_string()) {
            anyhow::bail!(
                "public_key {} встречается дважды",
                short_key(&peer.public_key)
            );
        }
    }
    Ok(())
}

/// Подсеть для MASQUERADE: `10.66.0.1/16` → `10.66.0.0/16`.
///
/// Считается от адреса интерфейса, а не берётся константой: пул адресов задаёт
/// панель, и захардкоженная 10.66.0.0/16 однажды разошлась бы с ним.
pub fn nat_subnet(section: &AwgSection) -> Option<String> {
    let (ip, prefix) = parse_cidr(&section.address_cidr)?;
    let masked = network_address(ip, prefix)?;
    Some(format!("{}/{}", masked, prefix))
}

/// public_key (hex, как его отдаёт UAPI) → user_tag.
///
/// Обратный индекс для учёта: интерфейс знает только ключи, а квоты и онлайн
/// панель ведёт по тегам. Ключи, которые не разобрались, в индекс не попадают —
/// их трафик не будет приписан никому, и это лучше, чем приписать его чужому.
pub fn tags_by_key_hex(section: &AwgSection) -> HashMap<String, String> {
    section
        .peers
        .iter()
        .filter_map(|p| {
            uapi::key_base64_to_hex(&p.public_key)
                .ok()
                .map(|hex| (hex, p.user_tag.clone()))
        })
        .collect()
}

/// Первые 8 символов ключа для логов. Целиком публичный ключ в журнал не пишем:
/// он однозначно идентифицирует подписку.
pub(crate) fn short_key(key: &str) -> String {
    key.chars().take(8).collect()
}

/// `10.66.0.1/16` → (IPv4, 16). Только IPv4: пул адресов у панели IPv4.
pub fn parse_cidr(value: &str) -> Option<(std::net::Ipv4Addr, u8)> {
    let (ip, prefix) = value.trim().split_once('/')?;
    let ip: std::net::Ipv4Addr = ip.trim().parse().ok()?;
    let prefix: u8 = prefix.trim().parse().ok()?;
    if prefix > 32 {
        return None;
    }
    Some((ip, prefix))
}

/// Адрес сети по адресу и префиксу.
fn network_address(ip: std::net::Ipv4Addr, prefix: u8) -> Option<std::net::Ipv4Addr> {
    if prefix > 32 {
        return None;
    }
    let mask: u32 = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix as u32)
    };
    Some(std::net::Ipv4Addr::from(u32::from(ip) & mask))
}

/// Состояние AWG на узле между циклами агента.
///
/// Живёт в `AgentState`, поэтому дельты трафика переживают цикл, но не
/// перезапуск агента — после перезапуска первый замер честно считается «с
/// нуля», ровно так же, как это уже сделано для sing-box.
pub struct AwgManager {
    /// Раздел, который панель прислала последним.
    desired: Option<AwgSection>,
    /// Был ли этот раздел уже успешно применён.
    applied: bool,
    /// Накопительные счётчики по публичным ключам (hex) с прошлого замера.
    last_totals: HashMap<String, (u64, u64)>,
    /// Путь к бинарю, когда он уже проверен в этом процессе.
    binary: Option<PathBuf>,
    /// Сколько раз подряд применение падало — чтобы одинаковая ошибка была
    /// видна как нарастающая проблема, а не как ровный шум в журнале.
    failures: u32,
}

impl Default for AwgManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AwgManager {
    pub fn new() -> Self {
        Self {
            desired: None,
            applied: false,
            last_totals: HashMap::new(),
            binary: None,
            failures: 0,
        }
    }

    /// Запомнить раздел из свежего ответа панели.
    ///
    /// Отдельно от применения: конфиг узла читается и по расписанию, и по
    /// сигналу, а применять имеет смысл один раз за цикл.
    pub fn set_desired(&mut self, section: Option<AwgSection>) {
        if self.desired != section {
            self.applied = false;
        }
        self.desired = section;
    }

    /// Объявлен ли AWG на узле прямо сейчас.
    pub fn is_enabled(&self) -> bool {
        self.desired.as_ref().is_some_and(|s| s.enabled)
    }

    /// Довести реальное состояние узла до желаемого.
    ///
    /// Идемпотентна и вызывается каждую проверку конфига: интерфейс может
    /// пропасть и без участия панели (перезагрузка узла, убитый процесс), и
    /// узел обязан поднять его сам, а не ждать изменения конфига.
    pub async fn ensure_applied(&mut self, client: &reqwest::Client) {
        let Some(desired) = self.desired.clone() else {
            return;
        };

        if !desired.enabled {
            if !self.applied {
                info!("AWG: выключен панелью — гашу интерфейс {AWG_INTERFACE}");
                iface::bring_down(AWG_INTERFACE).await;
                self.last_totals.clear();
                self.applied = true;
            }
            return;
        }

        if let Err(e) = validate(&desired) {
            self.failures = self.failures.saturating_add(1);
            warn!(
                "AWG: раздел от панели невалиден ({e}) — не применяю (попытка {})",
                self.failures
            );
            return;
        }

        match self.apply(client, &desired).await {
            Ok(()) => {
                if self.failures > 0 {
                    info!("AWG: конфигурация применена после {} неудач", self.failures);
                }
                self.failures = 0;
                self.applied = true;
            }
            Err(e) => {
                self.failures = self.failures.saturating_add(1);
                self.applied = false;
                warn!(
                    "AWG: применить конфигурацию не удалось ({e}), попытка {}",
                    self.failures
                );
            }
        }
    }

    async fn apply(
        &mut self,
        client: &reqwest::Client,
        desired: &AwgSection,
    ) -> anyhow::Result<()> {
        // 1. Бинарь. Проверка sha256 обязательна, поэтому путь кешируется на
        //    процесс: считать хеш двадцати мегабайт каждые две минуты незачем.
        let binary = match &self.binary {
            Some(path) if path.exists() => path.clone(),
            _ => {
                let path = download::ensure_amneziawg_go(client).await?;
                self.binary = Some(path.clone());
                path
            }
        };

        // 2. Процесс. amneziawg-go демонизируется сам и переживает перезапуск
        //    агента — при самообновлении узла туннели не рвутся.
        let socket = match uapi::find_socket(AWG_INTERFACE) {
            Some(path) => path,
            None => {
                iface::spawn_amneziawg(&binary, AWG_INTERFACE).await?;
                uapi::wait_for_socket(AWG_INTERFACE, Duration::from_secs(10))
                    .await
                    .ok_or_else(|| anyhow::anyhow!("UAPI-сокет так и не появился"))?
            }
        };

        // 3. Текущее состояние — чтобы слать в UAPI только отличия. Полная
        //    перезапись пиров оборвала бы живые сессии всем разом.
        let current = match uapi::get_device(&socket).await {
            Ok(state) => Some(state),
            Err(e) => {
                warn!("AWG: get=1 не отработал ({e}) — применяю конфигурацию целиком");
                None
            }
        };

        let request = uapi::build_set_request(current.as_ref(), desired)?;
        if request.lines().count() > 1 {
            uapi::set_device(&socket, &request).await?;
            info!(
                "AWG: конфигурация синхронизирована, пиров {}",
                desired.peers.len()
            );
        }

        // 4. Адрес, MTU, форвардинг, NAT и порт — идемпотентно, как это уже
        //    сделано для портов sing-box в sync_firewall.
        iface::ensure_address(AWG_INTERFACE, &desired.address_cidr).await;
        iface::ensure_up(AWG_INTERFACE, AWG_MTU).await;
        if let Some(subnet) = nat_subnet(desired) {
            iface::ensure_forwarding().await;
            iface::ensure_netfilter(AWG_INTERFACE, &subnet, desired.listen_port).await;
        }
        iface::ensure_udp_port(desired.listen_port).await;
        Ok(())
    }

    /// Снять статистику по AWG-пирам и превратить её в дельты.
    ///
    /// Пустой результат, когда AWG выключен или сокет недоступен: «ничего не
    /// известно» здесь безопаснее, чем выдуманные нули, — панель по этому
    /// списку показывает онлайн.
    pub async fn collect_usage(&mut self) -> Vec<stats::PeerUsage> {
        if !self.is_enabled() {
            return Vec::new();
        }
        let Some(desired) = self.desired.as_ref() else {
            return Vec::new();
        };
        let Some(socket) = uapi::find_socket(AWG_INTERFACE) else {
            return Vec::new();
        };
        let device = match uapi::get_device(&socket).await {
            Ok(d) => d,
            Err(e) => {
                tracing::debug!("AWG: статистика не снята ({e})");
                return Vec::new();
            }
        };
        let tags = tags_by_key_hex(desired);
        stats::fold_deltas(
            &mut self.last_totals,
            &device.peers,
            &tags,
            stats::now_unix(),
            ONLINE_WINDOW_SECS,
        )
    }
}

#[cfg(test)]
pub(crate) fn test_section() -> AwgSection {
    AwgSection {
        enabled: true,
        listen_port: 51820,
        // 32 нулевых байта: валидный по длине ключ, больше от него в тестах
        // ничего не требуется.
        private_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
        address_cidr: "10.66.0.1/16".to_string(),
        jc: 4,
        jmin: 40,
        jmax: 70,
        s1: 30,
        s2: 40,
        h1: 1_148_476,
        h2: 2_148_476,
        h3: 3_148_476,
        h4: 4_148_476,
        peers: vec![AwgPeer {
            public_key: "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWY=".to_string(),
            allowed_ip: "10.66.0.7/32".to_string(),
            user_tag: "user_42".to_string(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caramba_shared::config::ConfigResponse;

    /// Общая с панелью фикстура контракта.
    ///
    /// Файл один на обе стороны: его же читает тест
    /// `awg_service::tests::awg_contract_fixture` в панели. Разойтись молча
    /// половины теперь не могут — любая правка имён, типов или единиц ломает
    /// тест у того, кто разошёлся.
    fn contract_fixture() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../libs/caramba-shared/testdata/awg_node_section.json"
        );
        let raw = std::fs::read_to_string(path).expect("фикстура контракта AWG");
        serde_json::from_str(&raw).expect("фикстура — валидный JSON")
    }

    /// Раздел `awg` из общей фикстуры разбирается узлом и проходит проверку.
    ///
    /// Это и есть тот самый JSON, который панель кладёт в ответ
    /// `GET /api/v2/node/config`: узел обязан не просто его распарсить, а
    /// принять к применению — иначе AWG на ноде молча не поднимется.
    #[test]
    fn contract_fixture_is_accepted_by_the_node() {
        let fixture = contract_fixture();
        let resp: ConfigResponse =
            serde_json::from_value(fixture["config_response"].clone()).expect("ConfigResponse");

        assert_eq!(resp.hash, "2d8364b4c0de4f118a7a2f2a9b1c33ee");
        let section = resp.awg.expect("раздел awg");
        assert!(resp.awg_hash.is_some(), "awg_hash обязан ехать с разделом");

        validate(&section).expect("раздел из фикстуры обязан проходить валидацию");

        // Единицы и смысл полей, а не только имена: порт — UDP-порт awg0,
        // адрес — адрес интерфейса с маской ПУЛА, а не /32.
        assert_eq!(section.listen_port, 17400);
        assert_eq!(section.address_cidr, "10.66.0.1/16");
        assert_eq!(nat_subnet(&section).as_deref(), Some("10.66.0.0/16"));

        // h4 намеренно больше i32::MAX: панель хранит H в BIGINT, и сужение
        // до i32 обрезало бы параметр обфускации в мусор.
        assert_eq!(section.h4, 4_148_476_000);
        assert!(section.h4 > i32::MAX as u32);

        // Ключи обязаны быть 32-байтовыми X25519 в base64 — UAPI принимает
        // только hex, перевод делает узел.
        let server_hex = uapi::key_base64_to_hex(&section.private_key).expect("private_key");
        assert_eq!(server_hex.len(), 64);

        // Обратный индекс ключ → тег: по нему узел приписывает трафик.
        let tags = tags_by_key_hex(&section);
        assert_eq!(tags.len(), 2);
        for peer in &section.peers {
            let hex = uapi::key_base64_to_hex(&peer.public_key).expect("public_key пира");
            assert_eq!(
                tags.get(&hex).map(String::as_str),
                Some(peer.user_tag.as_str())
            );
            assert!(
                peer.allowed_ip.ends_with("/32"),
                "пир обязан приходить с /32, иначе маршрут перекроет весь пул: {}",
                peer.allowed_ip
            );
        }
        // Отрицательная идентичность (аккаунт без Telegram) — штатный тег.
        assert!(section.peers.iter().any(|p| p.user_tag == "user_-46"));
    }

    /// Heartbeat из той же фикстуры: узел обязан уметь собрать ровно это тело,
    /// а панель — его принять.
    #[test]
    fn contract_fixture_heartbeat_round_trips() {
        use caramba_shared::api::HeartbeatRequest;

        let fixture = contract_fixture();
        let hb: HeartbeatRequest =
            serde_json::from_value(fixture["heartbeat_request"].clone()).expect("HeartbeatRequest");

        let active = hb.active_users.as_ref().expect("active_users");
        assert_eq!(active.len(), 3);

        // Порядок — байтовый по тегу: узел сортирует список перед отправкой.
        let tags: Vec<&str> = active.iter().map(|u| u.tag.as_str()).collect();
        let mut sorted = tags.clone();
        sorted.sort_unstable();
        assert_eq!(
            tags, sorted,
            "active_users обязан приходить отсортированным"
        );

        // Молчащий, но живой AWG-пир: нулевые дельты при online = true. Это и
        // есть причина, по которой онлайн нельзя выводить из трафика.
        let silent = active.iter().find(|u| u.tag == "user_-46").unwrap();
        assert_eq!((silent.rx_delta, silent.tx_delta), (0, 0));
        assert!(silent.online);

        // Трафик в active_users — справка. Квота считается из user_usage, и
        // там сумма ОБОИХ направлений; молчащего пира в ней нет вовсе.
        let usage = hb.user_usage.as_ref().expect("user_usage");
        let chatty = active.iter().find(|u| u.tag == "user_42").unwrap();
        assert_eq!(
            usage.get("user_42").copied(),
            Some(chatty.rx_delta + chatty.tx_delta)
        );
        assert!(!usage.contains_key("user_-46"));

        // Обратно в JSON — без потерь: тело едет на панель именно так.
        let back: HeartbeatRequest =
            serde_json::from_str(&serde_json::to_string(&hb).unwrap()).unwrap();
        assert_eq!(back.active_users.unwrap().len(), 3);
    }

    /// Эталонный раздел из контракта обязан проходить проверку — иначе панель
    /// и узел разойдутся в первый же день.
    #[test]
    fn the_contract_shaped_section_is_accepted() {
        validate(&test_section()).unwrap();
    }

    /// Ответ панели разбирается ровно из того JSON, который зафиксирован
    /// контрактом: раздел `awg` лежит РЯДОМ с sing-box-конфигом и едет со
    /// своим хешем.
    #[test]
    fn the_contract_json_deserializes_field_for_field() {
        let json = r#"{
            "hash": "2d8364b4c0de4f118a7a2f2a9b1c33ee",
            "content": {"log": {}},
            "awg_hash": "9f1c0b7d2e3a4b5c6d7e8f9012345678",
            "awg": {
                "enabled": true,
                "listen_port": 51820,
                "private_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                "address_cidr": "10.66.0.1/16",
                "jc": 4, "jmin": 40, "jmax": 70, "s1": 30, "s2": 40,
                "h1": 1148476, "h2": 2148476, "h3": 3148476, "h4": 4148476,
                "peers": [
                    {"public_key": "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWY=",
                     "allowed_ip": "10.66.0.7/32",
                     "user_tag": "user_42"}
                ]
            }
        }"#;
        let parsed: ConfigResponse = serde_json::from_str(json).unwrap();
        let section = parsed.awg.expect("раздел awg обязан разобраться");
        assert_eq!(section, test_section());
        assert_eq!(
            parsed.awg_hash.as_deref(),
            Some("9f1c0b7d2e3a4b5c6d7e8f9012345678")
        );
        validate(&section).unwrap();
    }

    /// Панель без AWG (старая версия или узел без тумблера) раздела не
    /// присылает вовсе — это не ошибка разбора, а «AWG не управляется».
    #[test]
    fn a_response_without_the_section_still_parses() {
        let parsed: ConfigResponse =
            serde_json::from_str(r#"{"hash":"abc","content":{}}"#).unwrap();
        assert!(parsed.awg.is_none());
        assert!(parsed.awg_hash.is_none());
    }

    /// H1..H4 заменяют штатные типы пакетов WireGuard 1..4: совпасть с ними
    /// нельзя, иначе обе стороны перестанут различать хендшейк и транспорт.
    #[test]
    fn reserved_header_types_are_rejected() {
        let mut s = test_section();
        s.h3 = 4;
        assert!(validate(&s).is_err());
    }

    /// Одинаковые H-параметры ломают разбор так же, как зарезервированные.
    #[test]
    fn duplicate_header_types_are_rejected() {
        let mut s = test_section();
        s.h2 = s.h4;
        assert!(validate(&s).is_err());
    }

    /// Ключ не из 32 байт — чужой формат, а не «почти правильный ключ».
    #[test]
    fn a_key_of_the_wrong_length_is_rejected() {
        let mut s = test_section();
        s.private_key = "c2hvcnQ=".to_string();
        assert!(validate(&s).is_err());
    }

    /// Пир без тега невозможно учесть: трафик уйдёт в никуда, лимит не
    /// сработает. Такой раздел лучше не применять целиком.
    #[test]
    fn a_peer_without_a_tag_is_rejected() {
        let mut s = test_section();
        s.peers[0].user_tag = "  ".to_string();
        assert!(validate(&s).is_err());
    }

    /// Два пира с одним ключом — одна подписка, выданная дважды: второй
    /// перетрёт первого в UAPI, и один из пользователей молча пропадёт.
    #[test]
    fn a_duplicate_public_key_is_rejected() {
        let mut s = test_section();
        let dup = s.peers[0].clone();
        s.peers.push(dup);
        assert!(validate(&s).is_err());
    }

    /// Нулевой порт — интерфейс поднимется и не будет слушать ничего.
    #[test]
    fn a_zero_listen_port_is_rejected() {
        let mut s = test_section();
        s.listen_port = 0;
        assert!(validate(&s).is_err());
    }

    /// Подсеть для MASQUERADE считается от адреса интерфейса.
    #[test]
    fn the_nat_subnet_is_the_network_of_the_interface_address() {
        let mut s = test_section();
        assert_eq!(nat_subnet(&s).as_deref(), Some("10.66.0.0/16"));
        s.address_cidr = "10.77.13.1/24".to_string();
        assert_eq!(nat_subnet(&s).as_deref(), Some("10.77.13.0/24"));
        s.address_cidr = "192.168.5.66/22".to_string();
        assert_eq!(nat_subnet(&s).as_deref(), Some("192.168.4.0/22"));
    }

    /// Обратный индекс ключ → тег: без него статистика UAPI не привязывается
    /// к людям вообще.
    #[test]
    fn peers_are_indexed_by_their_public_key_in_hex() {
        let tags = tags_by_key_hex(&test_section());
        assert_eq!(
            tags.get("4142434445464748494a4b4c4d4e4f505152535455565758595a616263646566")
                .map(String::as_str),
            Some("user_42")
        );
    }

    /// Выключенный раздел не требует валидных полей: гасить интерфейс можно
    /// всегда, даже когда панель уже обнулила ключи.
    #[test]
    fn a_disabled_section_is_not_enabled() {
        let mut mgr = AwgManager::new();
        mgr.set_desired(Some(AwgSection::default()));
        assert!(!mgr.is_enabled());
    }

    /// Новый состав пиров обязан снова пройти применение, иначе изменение
    /// зависло бы до перезапуска агента.
    #[test]
    fn a_changed_section_is_marked_for_reapply() {
        let mut mgr = AwgManager::new();
        mgr.set_desired(Some(test_section()));
        mgr.applied = true;
        let mut changed = test_section();
        changed.peers.clear();
        mgr.set_desired(Some(changed));
        assert!(
            !mgr.applied,
            "изменение раздела обязано снять признак применённости"
        );
    }

    /// Тот же самый раздел повторно не применяется: панель отдаёт конфиг
    /// каждые две минуты, и дёргать интерфейс на каждый ответ незачем.
    #[test]
    fn an_identical_section_keeps_the_applied_flag() {
        let mut mgr = AwgManager::new();
        mgr.set_desired(Some(test_section()));
        mgr.applied = true;
        mgr.set_desired(Some(test_section()));
        assert!(mgr.applied);
    }
}

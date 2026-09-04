//! Каталог CSM/1 и его части (`03-WIRE.md` 8.2, 8.2.1, 8.4, раздел 11).
//!
//! Каталог это единственный документ, который адресуется по своему хэшу:
//! `chash = sha256(кадр)` публикуется в корневом ключевом документе и
//! называется в каждой директиве. Отсюда два обязательства панели, которые
//! этот модуль берёт на себя целиком, а не оставляет вызывающему:
//!
//! - **Порядок массивов является функцией содержимого**, а не порядка строк в
//!   базе (8.2). Иначе два процесса панели подпишут один тир в разные байты,
//!   и опубликованный хэш тира протухнет при первом перезапуске. Поэтому
//!   `to_value` и `content_digest` работают с нормализованной копией: массивы
//!   отсортированы по первичному идентификатору как сырым байтам, что и есть
//!   рекомендуемый порядок спецификации.
//! - **Дайджест содержимого** (1.5) считается по модели тира и ни по чему
//!   больше: ни по часам, ни по запросившему, ни по идентификатору строки.
//!   Панель хранит подписанный кадр под `(tier, content_digest)` и
//!   переподписывает тир только когда дайджест изменился.
//!
//! Нарезка на части (8.4) режет готовый КАДР каталога, а не payload, и каждая
//! часть подписывается отдельно. Нарезка универсальна: каталог из одной части
//! идёт тем же путём, ветки «без нарезки» не существует.
//!
//! Набивка (12.3) для каталога и частей вытягивается один раз при подписи, а
//! не на запрос: части адресуются по содержимому, и набивка на запрос меняла
//! бы `chash`. Генератор корпуса не набивает, поэтому у подписи два входа:
//! [`Catalog::sign`] без набивки, которым идёт гейт, и [`Catalog::sign_padded`],
//! которым идёт панель.
//!
//! Гейт совместимости: `tests/csm_catalog.rs` воспроизводит эталонные кадры
//! корпуса `c1_min.bin` и `c1c_min_0.bin` байт в байт.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};

use super::cbor::{self, Value};
use super::directive::{Capabilities, PRESET_VOCABULARY, RelayResolution, Selection};
use super::docs::{DohEntry, LIFETIME_CATALOG, Mirror, SPEC_VERSION};
use super::frame::{self, DocType, FrameError, MAX_PAYLOAD_LEN};
use super::pad::{PadError, pad_to_bucket};

/// Размер среза кадра в одной части (`03-WIRE.md` 11.3): `11 * 256`, чтобы
/// кадр части с запасом на заголовки HTTP укладывался под `RESP_MAX`.
pub const CHUNK_PAYLOAD_MAX: usize = 2816;

/// Потолок кадра части после набивки.
pub const CHUNK_RESP_MAX: usize = 3584;

/// Максимум частей: поле `n` ограничено 1..64.
pub const MAX_CHUNKS: usize = 64;

/// Порог предупреждения: выше него панель обязана залогировать тенанта и
/// число узлов (`01-DECISION.md` 5.2.7).
pub const PANEL_WARN: usize = 12288;

/// Порог отказа: payload выше него панель НЕ подписывает (инвариант 6).
pub const PANEL_REFUSE: usize = 49152;

/// Разделитель домена для дайджеста содержимого: тот же CBOR, что и в payload,
/// не должен случайно совпасть с хэшем чего-то другого.
const CONTENT_DIGEST_DOMAIN: &[u8] = b"csm1-catalog-content\0";

/// Разделитель домена ключа хранения: дайджест содержимого, связанный с
/// тенантом и подписантом.
const STORAGE_DIGEST_DOMAIN: &[u8] = b"csm1-catalog-storage\0";

/// Биты возможностей оператора (`03-WIRE.md` 5.1). Поле `cap` это bstr(4),
/// то есть u32 в big-endian. Значения берутся из [`Capabilities`] директивы:
/// `02-SPEC.md` 6.5 требует, чтобы каталог и директива согласовывали биты, и
/// одно определение даёт это по построению.
pub mod cap {
    use super::Capabilities;

    /// Материал подключения per-node присутствует в каталоге (BC1).
    pub const NODE_MATERIAL: u32 = Capabilities::NODE_MATERIAL;
    /// Доступны запечатанные директивы (HPKE, `0x06`).
    pub const SEALED_DIRECTIVES: u32 = Capabilities::SEALED_DIRECTIVES;
    /// Цепочки через релэй реальны: ставится только когда генератор Clash их
    /// действительно выпускает (P8) — не когда флот их допускает. Наличие
    /// записей `re` и ссылок `rl` условие необходимое, но не достаточное, и
    /// [`Catalog::validate`] эти два поля намеренно не связывает: бит
    /// описывает деплой оператора, а не содержимое кадра. Решение принимает
    /// панель, `csm::catalog_store::implemented_capabilities`.
    ///
    /// [`Catalog::validate`]: super::Catalog::validate
    pub const RELAY_CHAINING: u32 = Capabilities::RELAY_CHAINING;
    /// Доступна запись настроек.
    pub const SETTINGS_WRITE: u32 = Capabilities::SETTINGS_WRITE;
    /// Подписанный пул зеркал присутствует.
    pub const MIRROR_POOL: u32 = Capabilities::MIRROR_POOL;
    /// DoH-точки присутствуют в каталоге.
    pub const DOH: u32 = Capabilities::DOH;
    /// Хэши целостности rule-set и geo присутствуют (C3).
    pub const RESOURCE_HASHES: u32 = Capabilities::RESOURCE_HASHES;
    /// Канал устаревания присутствует (B7).
    pub const DEPRECATION: u32 = Capabilities::DEPRECATION_CHANNEL;
    /// Доступен onboarding-грант трафика.
    pub const ONBOARDING_GRANT: u32 = Capabilities::ONBOARDING_GRANT;
    /// Доступна регистрация ключа устройства.
    pub const DEVICE_ENROLLMENT: u32 = Capabilities::DEVICE_ENROLLMENT;
    /// `variant` пробрасывается насквозь через `caramba-sub` (P4).
    pub const VARIANT_FORWARDED: u32 = Capabilities::VARIANT_FORWARDED;
    /// Флот поддерживает port hopping.
    pub const PORT_HOPPING: u32 = Capabilities::PORT_HOPPING;
    /// Биты 12..31 зарезервированы: подписант обязан выпускать нули.
    pub const RESERVED: u32 = !Capabilities::DEFINED;
}

/// Протокол узла, `pr` (`03-WIRE.md` раздел 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Protocol {
    Vless = 1,
    Vmess = 2,
    Trojan = 3,
    Hysteria2 = 4,
    Tuic = 5,
    Shadowsocks = 6,
    Naive = 7,
    /// Inbound `amneziawg` панели, отрисовывается как wireguard outbound.
    Wireguard = 8,
}

/// Транспорт, `nw`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Network {
    Tcp = 1,
    Ws = 2,
    Grpc = 3,
    HttpUpgrade = 4,
    /// Эквивалентно `splithttp`. `02-SPEC.md` 4.4.1 запрещает выпускать такую
    /// запись в каталог для mihomo: клиент не сможет её набрать.
    Xhttp = 5,
    Quic = 6,
}

/// Безопасность транспорта, `se`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Security {
    None = 0,
    Tls = 1,
    Reality = 2,
}

/// Отпечаток uTLS, `fp`. По умолчанию у клиента `chrome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Fingerprint {
    Chrome = 1,
    Firefox = 2,
    Safari = 3,
    Ios = 4,
    Android = 5,
    Edge = 6,
    Qihoo360 = 7,
    Qq = 8,
    Random = 9,
    Randomized = 10,
}

/// VLESS flow, `fl`. Значение 0 в спецификации означает «ключ отсутствует», и
/// здесь оно выражено как `None` у поля: пустой `flow` ломает Happ, поэтому
/// рендерер обязан опускать ключ целиком.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Flow {
    Vision = 1,
}

/// Запись ALPN, `alp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u64)]
pub enum Alpn {
    H2 = 1,
    Http11 = 2,
    H3 = 3,
}

/// Управление перегрузкой TUIC, `cg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Congestion {
    Bbr = 1,
    Cubic = 2,
    NewReno = 3,
}

/// Метод Shadowsocks, `ssm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SsMethod {
    Blake3Aes128Gcm = 1,
    Blake3Aes256Gcm = 2,
    Blake3Chacha20Poly1305 = 3,
    Aes128Gcm = 4,
    Aes256Gcm = 5,
    Chacha20IetfPoly1305 = 6,
}

/// Ступень лестницы, `rung` (`01-DECISION.md` 5.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u64)]
pub enum Rung {
    /// Кэшированные подписанные документы.
    Cached = 0,
    /// Прямой HTTPS к origin энроллмента.
    Direct = 1,
    /// Подписанные зеркала.
    Mirrors = 2,
    /// Адрес через DoH с явным SNI.
    Doh = 3,
    /// Через собственный туннель приложения.
    Tunnel = 4,
    /// Прокси SOCKS5/HTTP, введённый пользователем.
    UserProxy = 5,
    /// Вне канала. Никогда не отключается.
    OutOfBand = 6,
}

/// Запись узла (`03-WIRE.md` 8.2.1). Обязательные поля это кортеж
/// подключения; всё остальное зависит от протокола, и отсутствующее поле не
/// выпускается вовсе, а не выпускается пустым.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// `n<node_id>i<inbound_id>`, 1..24 символов `[0-9A-Za-z_-]`. Единственный
    /// безопасный ключ узла: `pn` не уникален.
    pub id: String,
    /// Имя прокси mihomo дословно, как его выпускает генератор Clash.
    pub pn: String,
    /// Страна, две заглавные буквы ISO 3166-1.
    pub cc: String,
    /// `frontend_url`, если задан, иначе адрес узла. Может быть IP-литералом.
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub network: Network,
    pub security: Security,
    pub sni: Option<String>,
    /// Публичный ключ Reality СЫРЫМИ байтами, не base64url.
    pub public_key: Option<[u8; 32]>,
    /// Reality short id, hex.
    pub short_id: Option<String>,
    pub fingerprint: Option<Fingerprint>,
    pub flow: Option<Flow>,
    /// Путь WS или имя сервиса gRPC.
    pub path: Option<String>,
    /// Заголовок Host; опускается, когда равен `sni`.
    pub host_header: Option<String>,
    pub alpn: Vec<Alpn>,
    /// Диапазон портов Hysteria2 для port hopping.
    pub hop_ports: Option<String>,
    /// Обфускация Hysteria2.
    pub obfs: Option<String>,
    pub congestion: Option<Congestion>,
    /// TUIC zero-RTT. `Some(false)` выпускается явно, `None` опускается.
    pub zero_rtt: Option<bool>,
    /// skip-cert-verify. `Some(false)` выпускается явно, `None` опускается.
    pub insecure: Option<bool>,
    /// `id` записи в `re`, через которую этот выход идёт цепочкой.
    pub relay: Option<String>,
    pub ss_method: Option<SsMethod>,
    /// MTU wireguard, 576..1500.
    pub mtu: Option<u64>,
}

impl Node {
    /// Запись из кортежа подключения; необязательные поля выставляются после.
    /// Восемь аргументов это ровно восемь обязательных полей 8.2.1: сворачивать
    /// их в структуру значило бы добавить тип ради счётчика линтера.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        pn: impl Into<String>,
        cc: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        protocol: Protocol,
        network: Network,
        security: Security,
    ) -> Node {
        Node {
            id: id.into(),
            pn: pn.into(),
            cc: cc.into(),
            host: host.into(),
            port,
            protocol,
            network,
            security,
            sni: None,
            public_key: None,
            short_id: None,
            fingerprint: None,
            flow: None,
            path: None,
            host_header: None,
            alpn: Vec::new(),
            hop_ports: None,
            obfs: None,
            congestion: None,
            zero_rtt: None,
            insecure: None,
            relay: None,
            ss_method: None,
            mtu: None,
        }
    }

    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(1, Value::Text(self.id.clone()));
        m.insert(2, Value::Text(self.pn.clone()));
        m.insert(3, Value::Text(self.cc.clone()));
        m.insert(4, Value::Text(self.host.clone()));
        m.insert(5, Value::Uint(self.port as u64));
        m.insert(6, Value::Uint(self.protocol as u64));
        m.insert(7, Value::Uint(self.network as u64));
        m.insert(8, Value::Uint(self.security as u64));
        if let Some(s) = &self.sni {
            m.insert(9, Value::Text(s.clone()));
        }
        if let Some(k) = &self.public_key {
            m.insert(10, Value::Bytes(k.to_vec()));
        }
        if let Some(s) = &self.short_id {
            m.insert(11, Value::Text(s.clone()));
        }
        if let Some(fp) = self.fingerprint {
            m.insert(12, Value::Uint(fp as u64));
        }
        if let Some(fl) = self.flow {
            m.insert(13, Value::Uint(fl as u64));
        }
        if let Some(p) = &self.path {
            m.insert(14, Value::Text(p.clone()));
        }
        if let Some(h) = &self.host_header {
            m.insert(15, Value::Text(h.clone()));
        }
        if !self.alpn.is_empty() {
            m.insert(
                16,
                Value::Array(self.alpn.iter().map(|a| Value::Uint(*a as u64)).collect()),
            );
        }
        if let Some(h) = &self.hop_ports {
            m.insert(17, Value::Text(h.clone()));
        }
        if let Some(o) = &self.obfs {
            m.insert(18, Value::Text(o.clone()));
        }
        if let Some(cg) = self.congestion {
            m.insert(19, Value::Uint(cg as u64));
        }
        if let Some(z) = self.zero_rtt {
            m.insert(20, Value::Bool(z));
        }
        if let Some(i) = self.insecure {
            m.insert(21, Value::Bool(i));
        }
        if let Some(r) = &self.relay {
            m.insert(22, Value::Text(r.clone()));
        }
        if let Some(s) = self.ss_method {
            m.insert(23, Value::Uint(s as u64));
        }
        if let Some(mtu) = self.mtu {
            m.insert(24, Value::Uint(mtu));
        }
        Value::Map(m)
    }

    /// Правила 8.2.1, относящиеся к одной записи. Публичный вход нужен
    /// панели: она отбрасывает негодный инбаунд по одному, а не теряет тир
    /// целиком, и правило отбора обязано быть тем же, по которому потом
    /// проверяется весь каталог. `role` попадает в текст ошибки: `ex` или `re`.
    pub fn validate(&self, role: &'static str) -> Result<(), CatalogError> {
        let ctx = |reason: &str| CatalogError::Invalid(format!("{role} {:?}: {reason}", self.id));
        if !is_node_id(&self.id) {
            return Err(ctx("id вне формата 1..24 символов [0-9A-Za-z_-]"));
        }
        if self.id == "default" {
            return Err(ctx("id равен сентинелу сброса `default`"));
        }
        if self.pn.is_empty() || self.pn.len() > 64 {
            return Err(ctx("pn пуст или длиннее 64 байт"));
        }
        if !is_country(&self.cc) {
            return Err(ctx("cc не две заглавные буквы"));
        }
        if !is_hostname(&self.host) && !is_ip_literal(&self.host) {
            return Err(ctx("h не hostname и не IP-литерал"));
        }
        if self.port == 0 {
            return Err(ctx("порт 0"));
        }
        if self.network == Network::Xhttp {
            // 02-SPEC.md 4.4.1: ни один рендерер клиента не наберёт xhttp,
            // а узел, который движок не может использовать, это та же ложь,
            // что бит возможности над отсутствующей функцией.
            return Err(ctx("nw = xhttp не выпускается в каталог"));
        }
        for (name, v, cap) in [("sni", &self.sni, 64usize), ("hst", &self.host_header, 64)] {
            if let Some(s) = v
                && !is_hostname(s)
            {
                return Err(ctx(&format!("{name} не hostname (<= {cap})")));
            }
        }
        // 8.2.1: `hst` опускается, когда равен `sni`. Избыточная форма дала
        // бы два кадра одного узла.
        if self.host_header.is_some() && self.host_header == self.sni {
            return Err(ctx("hst равен sni и обязан быть опущен"));
        }
        if let Some(s) = &self.short_id
            && (s.is_empty() || s.len() > 16 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(ctx("sid не hex длиной 1..16"));
        }
        if let Some(p) = &self.path
            && (p.is_empty() || p.len() > 96)
        {
            return Err(ctx("pt пуст или длиннее 96"));
        }
        if self.alpn.len() > 3 {
            return Err(ctx("alp длиннее 3"));
        }
        for (name, v) in [("hop", &self.hop_ports), ("obf", &self.obfs)] {
            if let Some(s) = v
                && (s.is_empty() || s.len() > 32)
            {
                return Err(ctx(&format!("{name} пуст или длиннее 32")));
            }
        }
        if let Some(r) = &self.relay
            && !is_node_id(r)
        {
            return Err(ctx("rl вне формата id"));
        }
        if let Some(mtu) = self.mtu
            && !(576..=1500).contains(&mtu)
        {
            return Err(ctx("mtu вне 576..1500"));
        }
        Ok(())
    }
}

/// Пресет маршрутизации (`ro`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Закрытый словарь, `[a-z0-9-]`, <= 32.
    pub id: String,
    /// Отображаемое имя, инертный текст.
    pub name: String,
    /// Имена ресурсов из `rs`.
    pub rulesets: Vec<String>,
}

impl Route {
    fn to_value(&self) -> Value {
        Value::map([
            (1, Value::Text(self.id.clone())),
            (2, Value::Text(self.name.clone())),
            (
                3,
                Value::Array(
                    self.rulesets
                        .iter()
                        .map(|r| Value::Text(r.clone()))
                        .collect(),
                ),
            ),
        ])
    }
}

/// Ресурс с хэшем (`rs`, `geo`): единственная целостность, которую кто-либо
/// даёт данным, решающим, какие пакеты идут в туннель.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// Имя провайдера, <= 48.
    pub name: String,
    /// Путь без хоста, <= 128 (`03-WIRE.md` 14.2).
    pub path: String,
    /// sha256 байтов файла.
    pub sha256: [u8; 32],
    /// Интервал обновления в секундах, 3600..604800.
    pub interval: Option<u64>,
}

impl Resource {
    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(1, Value::Text(self.name.clone()));
        m.insert(2, Value::Text(self.path.clone()));
        m.insert(3, Value::Bytes(self.sha256.to_vec()));
        if let Some(iv) = self.interval {
            m.insert(4, Value::Uint(iv));
        }
        Value::Map(m)
    }

    fn validate(&self, role: &'static str) -> Result<(), CatalogError> {
        let ctx = |reason: &str| CatalogError::Invalid(format!("{role} {:?}: {reason}", self.name));
        if self.name.is_empty() || self.name.len() > 48 {
            return Err(ctx("n пуст или длиннее 48"));
        }
        if !is_path(&self.path) {
            return Err(ctx("u не path-only (14.2)"));
        }
        if let Some(iv) = self.interval
            && !(3600..=604_800).contains(&iv)
        {
            return Err(ctx("iv вне 3600..604800"));
        }
        Ok(())
    }
}

/// Пины SPKI для хоста манифестов (`pin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub host: String,
    /// sha256 SPKI, 1..4.
    pub spki: Vec<[u8; 32]>,
}

impl Pin {
    fn to_value(&self) -> Value {
        Value::map([
            (1, Value::Text(self.host.clone())),
            (
                2,
                Value::Array(self.spki.iter().map(|p| Value::Bytes(p.to_vec())).collect()),
            ),
        ])
    }
}

/// Значения лестницы по умолчанию (`lad`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ladder {
    /// Порядок ступеней, 1..7 без дубликатов.
    pub order: Vec<Rung>,
    /// Включённые ступени, подмножество `order`. R0 и R6 обязательны.
    pub enabled: Vec<Rung>,
}

impl Ladder {
    fn to_value(&self) -> Value {
        Value::map([
            (
                1,
                Value::Array(self.order.iter().map(|r| Value::Uint(*r as u64)).collect()),
            ),
            (
                2,
                Value::Array(
                    self.enabled
                        .iter()
                        .map(|r| Value::Uint(*r as u64))
                        .collect(),
                ),
            ),
        ])
    }

    fn validate(&self) -> Result<(), CatalogError> {
        let err = |s: &str| CatalogError::Invalid(format!("lad: {s}"));
        if self.order.is_empty() || self.order.len() > 7 {
            return Err(err("ord вне 1..7"));
        }
        let ord: BTreeSet<Rung> = self.order.iter().copied().collect();
        if ord.len() != self.order.len() {
            return Err(err("ord содержит дубликат"));
        }
        let en: BTreeSet<Rung> = self.enabled.iter().copied().collect();
        if !en.is_subset(&ord) {
            return Err(err("en не подмножество ord"));
        }
        // R6 не отключается никогда, R0 это кэш, без которого нет офлайна.
        if !en.contains(&Rung::Cached) || !en.contains(&Rung::OutOfBand) {
            return Err(err("en обязан содержать R0 и R6"));
        }
        Ok(())
    }
}

/// Подписанные пороги размера (`thr`, `01-DECISION.md` инвариант 5). Это
/// поля каталога, а не константы компиляции: измерение на реальном пути
/// меняет их без выпуска клиента.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// Максимум учтённых байт на одном TCP-соединении, рукопожатие включено.
    pub conn_bytes: u64,
    /// Максимум учтённых пакетов с данными на одном соединении.
    pub conn_packets: u64,
    /// Максимум байт тела ответа.
    pub resp_max: u64,
}

impl Default for Thresholds {
    /// Значения по умолчанию из `03-WIRE.md` 8.2, выведенные в разделе 11.
    fn default() -> Self {
        Thresholds {
            conn_bytes: 8192,
            conn_packets: 22,
            resp_max: 4096,
        }
    }
}

impl Thresholds {
    fn to_value(self) -> Value {
        Value::map([
            (1, Value::Uint(self.conn_bytes)),
            (2, Value::Uint(self.conn_packets)),
            (3, Value::Uint(self.resp_max)),
        ])
    }
}

/// Ключ HPKE панели (`hpk`, `hpkv`): получатель запечатанных записей
/// клиента, но никогда не получатель `0x06`. Поколение обязано присутствовать
/// вместе с ключом, поэтому они одна структура.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HpkeKey {
    /// P-256 без сжатия, 65 байт.
    pub public_key: [u8; 65],
    /// Поколение, начиная с 1.
    pub generation: u64,
}

/// Каталог, `doc_type = 0x02`. Подписывается онлайн-ключом, побайтово одинаков
/// для каждого подписчика тира.
///
/// Массивы выпускаются в рекомендуемом порядке `03-WIRE.md` 8.2 независимо от
/// того, в каком порядке они заданы здесь: `id` для узлов и маршрутов, `h` для
/// зеркал, DoH и пинов, `n` для ресурсов, по возрастанию как сырые байты.
///
/// Без `PartialEq`: `docs::Mirror` и `docs::DohEntry` его не выводят, а
/// сравнивать модели надо по [`Catalog::content_digest`], не по полям.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub pid: [u8; 8],
    /// Счётчик изменений содержимого тира, не часы и не id строки.
    pub ver: u64,
    /// Момент изменения содержимого, не момент запроса.
    pub iat: u64,
    /// Тир плана, 1..1023.
    pub tier: u64,
    /// Выходные узлы, 1..512. Пустой список это ошибка подписи, а не состояние.
    pub exits: Vec<Node>,
    /// Релэи, 0..64. Узел из `relays` не может быть и в `exits`.
    pub relays: Vec<Node>,
    /// Пресеты маршрутизации, 0..32.
    pub routes: Vec<Route>,
    /// Битовое поле возможностей, см. [`cap`].
    pub cap: u32,
    /// Подписанный пул зеркал, 0..32.
    pub mirrors: Vec<Mirror>,
    /// DoH-точки, 0..8. Каждый `h` обязан быть и в `mirrors`.
    pub doh: Vec<DohEntry>,
    /// Провайдеры rule-set, 0..32.
    pub rulesets: Vec<Resource>,
    /// Базы geo, 0..8.
    pub geo: Vec<Resource>,
    /// Период обновления директивы в секундах, 300..86400.
    pub ttl: u64,
    /// Джиттер обновления в процентах от `ttl`, 0..50.
    pub jitter: u64,
    pub thresholds: Thresholds,
    /// Диапазон корзин набивки `[lo, hi]`, каждое 0..15, `lo <= hi`.
    pub pad_buckets: [u64; 2],
    pub ladder: Option<Ladder>,
    /// Пины SPKI хостов манифестов, 0..32.
    pub pins: Vec<Pin>,
    pub hpke: Option<HpkeKey>,
}

/// Ошибка сборки, проверки или подписи каталога.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// Модель нарушает правило спецификации; текст называет поле и причину.
    Invalid(String),
    /// Payload выше `PANEL_REFUSE`. Несёт число выходов, чтобы лог панели
    /// назвал его вместе с тенантом, как требует 11.3.
    TooLarge { payload_len: usize, exits: usize },
    /// Выход за пределы профиля CBOR.
    Limit(cbor::LimitError),
    /// Ошибка сборки кадра.
    Frame(FrameError),
    /// Кадр каталога режется на больше чем `MAX_CHUNKS` частей.
    TooManyChunks(usize),
    /// Кадр каталога длиннее `tl` части (8.4): ни один клиент его не соберёт.
    FrameTooLong(usize),
    /// Набивка не уместилась под потолок.
    Pad(PadError),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::Invalid(s) => write!(f, "csm catalog: {s}"),
            CatalogError::TooLarge { payload_len, exits } => write!(
                f,
                "csm catalog: payload {payload_len} байт при {exits} выходах выше \
                 PANEL_REFUSE {PANEL_REFUSE}"
            ),
            CatalogError::Limit(e) => write!(f, "csm catalog: {e}"),
            CatalogError::Frame(e) => write!(f, "csm catalog: {e}"),
            CatalogError::TooManyChunks(n) => {
                write!(f, "csm catalog: {n} частей выше предела {MAX_CHUNKS}")
            }
            CatalogError::FrameTooLong(n) => write!(
                f,
                "csm catalog: кадр в {n} байт длиннее предела tl {MAX_PAYLOAD_LEN}"
            ),
            CatalogError::Pad(e) => write!(f, "csm catalog: {e}"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<cbor::LimitError> for CatalogError {
    fn from(e: cbor::LimitError) -> Self {
        CatalogError::Limit(e)
    }
}

impl From<FrameError> for CatalogError {
    fn from(e: FrameError) -> Self {
        CatalogError::Frame(e)
    }
}

impl From<PadError> for CatalogError {
    fn from(e: PadError) -> Self {
        CatalogError::Pad(e)
    }
}

/// Результат подписи: кадр каталога, его хэш и уже подписанные части. Это
/// ровно то, что панель кладёт в `csm_catalogs` и `csm_catalog_chunks`, и
/// отдача потом становится чтением.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedCatalog {
    /// Полный кадр `0x02`.
    pub frame: Vec<u8>,
    /// `sha256(frame)`: публикуется в `tiers` ключевого документа и в `cat`
    /// директивы.
    pub chash: [u8; 32],
    /// Кадры `0x04` по индексу. Их число это `cn` директивы.
    pub chunks: Vec<Vec<u8>>,
    /// Длина payload каталога: панель сравнивает с [`PANEL_WARN`].
    pub payload_len: usize,
}

impl SignedCatalog {
    /// `cat_id`: base32 Crockford от `chash[0..10]`, сегмент пути `/sub/c1/`.
    pub fn cat_id(&self) -> String {
        cat_id(&self.chash)
    }

    /// Число частей, `cn` директивы.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Payload выше порога, при котором панель обязана залогировать тенанта и
    /// число узлов.
    pub fn exceeds_warn(&self) -> bool {
        self.payload_len > PANEL_WARN
    }
}

impl Catalog {
    /// Копия с массивами в рекомендуемом порядке 8.2. Используется всеми
    /// путями выпуска, чтобы порядок строк в базе не мог попасть в байты.
    fn normalized(&self) -> Catalog {
        let mut c = self.clone();
        c.sort_recommended();
        c
    }

    /// Сортирует массивы в рекомендуемом порядке 8.2 на месте. Выпуск делает
    /// это сам; метод публичен, чтобы панель могла сравнить модель с тем, что
    /// будет подписано.
    pub fn sort_recommended(&mut self) {
        self.exits
            .sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        self.relays
            .sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        self.routes
            .sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        self.mirrors
            .sort_by(|a, b| a.host.as_bytes().cmp(b.host.as_bytes()));
        self.doh
            .sort_by(|a, b| a.host.as_bytes().cmp(b.host.as_bytes()));
        self.pins
            .sort_by(|a, b| a.host.as_bytes().cmp(b.host.as_bytes()));
        self.rulesets
            .sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        self.geo
            .sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        for n in self.exits.iter_mut().chain(self.relays.iter_mut()) {
            n.alpn.sort();
        }
    }

    /// Поля содержимого, ключи 10..26: всё, что подписывается, кроме конверта.
    fn content_pairs(&self) -> BTreeMap<u64, Value> {
        let mut m = BTreeMap::new();
        m.insert(10, Value::Uint(self.tier));
        m.insert(
            11,
            Value::Array(self.exits.iter().map(Node::to_value).collect()),
        );
        if !self.relays.is_empty() {
            m.insert(
                12,
                Value::Array(self.relays.iter().map(Node::to_value).collect()),
            );
        }
        if !self.routes.is_empty() {
            m.insert(
                13,
                Value::Array(self.routes.iter().map(Route::to_value).collect()),
            );
        }
        m.insert(14, Value::Bytes(self.cap.to_be_bytes().to_vec()));
        if !self.mirrors.is_empty() {
            m.insert(
                15,
                Value::Array(self.mirrors.iter().map(mirror_value).collect()),
            );
        }
        if !self.doh.is_empty() {
            m.insert(16, Value::Array(self.doh.iter().map(doh_value).collect()));
        }
        if !self.rulesets.is_empty() {
            m.insert(
                17,
                Value::Array(self.rulesets.iter().map(Resource::to_value).collect()),
            );
        }
        if !self.geo.is_empty() {
            m.insert(
                18,
                Value::Array(self.geo.iter().map(Resource::to_value).collect()),
            );
        }
        m.insert(19, Value::Uint(self.ttl));
        m.insert(20, Value::Uint(self.jitter));
        m.insert(21, self.thresholds.to_value());
        m.insert(
            22,
            Value::Array(vec![
                Value::Uint(self.pad_buckets[0]),
                Value::Uint(self.pad_buckets[1]),
            ]),
        );
        if let Some(l) = &self.ladder {
            m.insert(23, l.to_value());
        }
        if !self.pins.is_empty() {
            m.insert(
                24,
                Value::Array(self.pins.iter().map(Pin::to_value).collect()),
            );
        }
        if let Some(k) = &self.hpke {
            m.insert(25, Value::Bytes(k.public_key.to_vec()));
            m.insert(26, Value::Uint(k.generation));
        }
        m
    }

    /// Полезная нагрузка без набивки: конверт плюс содержимое.
    fn to_map(&self) -> BTreeMap<u64, Value> {
        let c = self.normalized();
        let mut m = envelope(&c.pid, c.ver, c.iat, LIFETIME_CATALOG);
        m.append(&mut c.content_pairs());
        m
    }

    /// Значение полезной нагрузки: конверт плюс содержимое.
    pub fn to_value(&self) -> Value {
        Value::Map(self.to_map())
    }

    /// Дайджест содержимого тира (`03-WIRE.md` 1.5): ключ, под которым панель
    /// хранит подписанный кадр и по изменению которого переподписывает.
    ///
    /// Покрывает каждое поле содержимого, включая `cap`, `ttl`, `jit`, `pb`,
    /// `lad` и `hpk`: спецификация перечисляет узлы, релэи, маршруты, зеркала,
    /// DoH, ресурсы, пины и пороги, но исключить остальные поля значило бы,
    /// что смена бита возможности никогда не переподписывает каталог и не
    /// доходит до клиента. Исключён только конверт: `pid`, `ver`, `iat`, `exp`.
    pub fn content_digest(&self) -> [u8; 32] {
        let c = self.normalized();
        let body = cbor::encode(&Value::Map(c.content_pairs()));
        let mut h = Sha256::new();
        h.update(CONTENT_DIGEST_DOMAIN);
        h.update(&body);
        h.finalize().into()
    }

    /// Ключ, под которым панель хранит подписанный кадр: дайджест содержимого,
    /// связанный с `pid` и с `keyid_trunc` подписанта.
    ///
    /// Содержимое тира не меняется от ротации ключа, а хранимый кадр меняется:
    /// подпись отозванным `kid` не пройдёт V-role ни у одного клиента, и
    /// ключевой документ с новым `pid` делает старый конверт чужим. Ни то, ни
    /// другое не видно в [`Catalog::content_digest`] по определению 1.5, поэтому
    /// подписант входит в ключ хранения, а не в дайджест содержимого: ротация
    /// даёт ровно одну переподпись на тир, а флот без изменений по-прежнему
    /// не переподписывается.
    pub fn storage_digest(&self, signer_kid: &[u8; 12]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(STORAGE_DIGEST_DOMAIN);
        h.update(self.pid);
        h.update(signer_kid);
        h.update(self.content_digest());
        h.finalize().into()
    }

    /// Проверяет модель против правил 8.2, 8.2.1 и `02-SPEC.md` 4.4, которые
    /// не выражены типами. Вызывается из [`Catalog::encode`].
    pub fn validate(&self) -> Result<(), CatalogError> {
        let err = |s: &str| CatalogError::Invalid(s.to_string());
        if self.ver >= 1 << 32 {
            return Err(err("ver не меньше 2^32"));
        }
        if !(1..=1023).contains(&self.tier) {
            return Err(err("tier вне 1..1023"));
        }
        if self.exits.is_empty() {
            return Err(err("ex пуст: тир без выходов это ошибка подписи"));
        }
        if self.exits.len() > 512 {
            return Err(err("ex длиннее 512"));
        }
        if self.relays.len() > 64 {
            return Err(err("re длиннее 64"));
        }
        if self.routes.len() > 32 {
            return Err(err("ro длиннее 32"));
        }
        if self.mirrors.len() > 32 {
            return Err(err("mir длиннее 32"));
        }
        if self.doh.len() > 8 {
            return Err(err("doh длиннее 8"));
        }
        if self.rulesets.len() > 32 {
            return Err(err("rs длиннее 32"));
        }
        if self.geo.len() > 8 {
            return Err(err("geo длиннее 8"));
        }
        if self.pins.len() > 32 {
            return Err(err("pin длиннее 32"));
        }
        if self.cap & cap::RESERVED != 0 {
            return Err(err(
                "cap: зарезервированные биты 12..31 обязаны быть нулями",
            ));
        }
        if !(300..=86_400).contains(&self.ttl) {
            return Err(err("ttl вне 300..86400"));
        }
        if self.jitter > 50 {
            return Err(err("jit выше 50"));
        }
        let [lo, hi] = self.pad_buckets;
        if lo > 15 || hi > 15 || lo > hi {
            return Err(err("pb вне 0..15 или lo > hi"));
        }
        if let Some(k) = &self.hpke {
            if k.generation == 0 || k.generation >= 1 << 16 {
                return Err(err("hpkv вне 1..65535"));
            }
            // P-256 без сжатия начинается с 0x04; другой префикс это не тот
            // формат ключа, и клиент его отвергнет.
            if k.public_key[0] != 0x04 {
                return Err(err("hpk не P-256 uncompressed"));
            }
        }

        let mut exit_ids = BTreeSet::new();
        for n in &self.exits {
            n.validate("ex")?;
            if !exit_ids.insert(n.id.as_str()) {
                return Err(CatalogError::Invalid(format!("ex: дубликат id {:?}", n.id)));
            }
        }
        let mut relay_ids = BTreeSet::new();
        for n in &self.relays {
            n.validate("re")?;
            if !relay_ids.insert(n.id.as_str()) {
                return Err(CatalogError::Invalid(format!("re: дубликат id {:?}", n.id)));
            }
            if exit_ids.contains(n.id.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "узел {:?} одновременно в ex и re",
                    n.id
                )));
            }
            if n.relay.is_some() {
                return Err(CatalogError::Invalid(format!(
                    "re {:?}: релэй не может ссылаться на релэй",
                    n.id
                )));
            }
        }
        for n in &self.exits {
            if let Some(r) = &n.relay
                && !relay_ids.contains(r.as_str())
            {
                return Err(CatalogError::Invalid(format!(
                    "ex {:?}: rl {:?} не найден в re",
                    n.id, r
                )));
            }
        }

        let ruleset_names: BTreeSet<&str> = self.rulesets.iter().map(|r| r.name.as_str()).collect();
        if ruleset_names.len() != self.rulesets.len() {
            return Err(err("rs: дубликат имени"));
        }
        for r in &self.rulesets {
            r.validate("rs")?;
        }
        let geo_names: BTreeSet<&str> = self.geo.iter().map(|r| r.name.as_str()).collect();
        if geo_names.len() != self.geo.len() {
            return Err(err("geo: дубликат имени"));
        }
        for r in &self.geo {
            r.validate("geo")?;
        }

        let mut route_ids = BTreeSet::new();
        for r in &self.routes {
            // Закрытый словарь пресетов общий с `sel.preset` и `pol.preset`
            // директивы: пресет, которого нет в словаре ядра, клиент не
            // применит, а расхождение двух списков поймал бы только клиент.
            if r.id.is_empty() || r.id.len() > 32 || !PRESET_VOCABULARY.contains(&r.id.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "ro {:?}: id вне словаря пресетов ядра",
                    r.id
                )));
            }
            if !route_ids.insert(r.id.as_str()) {
                return Err(CatalogError::Invalid(format!("ro: дубликат id {:?}", r.id)));
            }
            if r.name.is_empty() || r.name.len() > 40 {
                return Err(CatalogError::Invalid(format!(
                    "ro {:?}: nm пуст или длиннее 40",
                    r.id
                )));
            }
            if r.rulesets.len() > 32 {
                return Err(CatalogError::Invalid(format!(
                    "ro {:?}: rs длиннее 32",
                    r.id
                )));
            }
            for name in &r.rulesets {
                if !ruleset_names.contains(name.as_str()) {
                    return Err(CatalogError::Invalid(format!(
                        "ro {:?}: ресурс {:?} не найден в rs",
                        r.id, name
                    )));
                }
            }
        }

        let mut mirror_hosts = BTreeSet::new();
        let mut asns = BTreeSet::new();
        let mut countries = BTreeSet::new();
        for m in &self.mirrors {
            validate_mirror(m)?;
            if !mirror_hosts.insert(m.host.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "mir: дубликат h {:?}",
                    m.host
                )));
            }
            asns.insert(m.asn);
            countries.insert(m.country.as_str());
        }
        // 01-DECISION.md D6: клиент проверяет заявленное разнообразие
        // провайдеров, значит панель обязана его обеспечить до подписи.
        if !self.mirrors.is_empty() && (asns.len() < 3 || countries.len() < 2) {
            return Err(err("mir: меньше трёх ASN или двух стран"));
        }
        let mut doh_hosts = BTreeSet::new();
        for d in &self.doh {
            validate_doh(d)?;
            if !doh_hosts.insert(d.host.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "doh: дубликат h {:?}",
                    d.host
                )));
            }
            if !mirror_hosts.contains(d.host.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "doh {:?}: хост обязан быть и в mir",
                    d.host
                )));
            }
        }
        let mut pin_hosts = BTreeSet::new();
        for p in &self.pins {
            if !is_hostname(&p.host) {
                return Err(CatalogError::Invalid(format!(
                    "pin {:?}: h не hostname",
                    p.host
                )));
            }
            if p.spki.is_empty() || p.spki.len() > 4 {
                return Err(CatalogError::Invalid(format!(
                    "pin {:?}: spki вне 1..4",
                    p.host
                )));
            }
            if !pin_hosts.insert(p.host.as_str()) {
                return Err(CatalogError::Invalid(format!(
                    "pin: дубликат h {:?}",
                    p.host
                )));
            }
        }
        if let Some(l) = &self.ladder {
            l.validate()?;
        }
        Ok(())
    }

    /// Два предиката `02-SPEC.md` 7.4, которые решаются только против
    /// каталога: `sel.exit` обязан называть запись `ex`, а `sel.relay` —
    /// запись `re`, чья страна равна `sel.rcc`.
    ///
    /// Клиенту эти два предиката запрещено проверять при разборе и запрещено
    /// отвергать по ним директиву: на первом запуске привязанного каталога
    /// у него ещё нет. Подписант в другом положении — у него на руках обе
    /// стороны, — и невыполнимый здесь выбор он выпускать не вправе, иначе
    /// клиент получит подписанную ссылку в никуда и молча свалится на
    /// умолчание оператора.
    ///
    /// Третий предикат, `sel.relay` при `rcc = --`, решается по одним байтам
    /// директивы и живёт в [`Selection::validate`](super::directive::Selection).
    pub fn check_selection(&self, sel: &Selection) -> Result<(), CatalogError> {
        if let Some(e) = &sel.exit
            && !self.exits.iter().any(|n| n.id == *e)
        {
            return Err(CatalogError::Invalid(format!(
                "sel.exit {e:?}: нет такой записи в ex"
            )));
        }
        let Some(r) = &sel.relay else {
            return Ok(());
        };
        let Some(entry) = self.relays.iter().find(|n| n.id == *r) else {
            return Err(CatalogError::Invalid(format!(
                "sel.relay {r:?}: нет такой записи в re"
            )));
        };
        let cc = match sel.rcc {
            RelayResolution::Country(cc) => cc,
            RelayResolution::NoRelay => {
                return Err(CatalogError::Invalid(format!(
                    "sel.relay {r:?} при rcc = --"
                )));
            }
        };
        if entry.cc.as_bytes() != cc {
            return Err(CatalogError::Invalid(format!(
                "sel.relay {r:?}: страна записи {:?} не равна rcc {:?}",
                entry.cc,
                String::from_utf8_lossy(&cc)
            )));
        }
        Ok(())
    }

    /// Кодирует payload: проверка модели, пределы профиля, порог отказа.
    ///
    /// Порог отказа проверяется здесь, а не в сборке кадра: кадр отверг бы
    /// длину молча как «вне диапазона», а 11.3 требует назвать число узлов.
    pub fn encode(&self) -> Result<Vec<u8>, CatalogError> {
        self.validate()?;
        let v = self.to_value();
        cbor::check(&v)?;
        let payload = cbor::encode(&v);
        if payload.len() > PANEL_REFUSE {
            return Err(CatalogError::TooLarge {
                payload_len: payload.len(),
                exits: self.exits.len(),
            });
        }
        Ok(payload)
    }

    /// Подписывает каталог без набивки и режет кадр на подписанные части.
    /// Это путь генератора корпуса, и им идёт байтовый гейт.
    pub fn sign(&self, signers: &[SigningKey]) -> Result<SignedCatalog, CatalogError> {
        self.sign_with(signers, None)
    }

    /// Подписывает каталог с набивкой на сетку (`03-WIRE.md` 12.2, 12.3):
    /// `bucket` это `r`, вытянутый один раз на эту подпись из `pb` каталога,
    /// общий для кадра каталога и всех его частей. Кадр каталога зажимается
    /// под `PANEL_REFUSE`, каждая часть под `CHUNK_RESP_MAX`. Это путь панели:
    /// результат целиком уходит в хранилище и больше не пересобирается.
    pub fn sign_padded(
        &self,
        signers: &[SigningKey],
        bucket: u32,
    ) -> Result<SignedCatalog, CatalogError> {
        self.sign_with(signers, Some(bucket))
    }

    fn sign_with(
        &self,
        signers: &[SigningKey],
        bucket: Option<u32>,
    ) -> Result<SignedCatalog, CatalogError> {
        let payload = self.encode()?;
        // Предел `tl` части (8.4) это длина КАДРА, а не payload: payload под
        // `PANEL_REFUSE` с подписями сверху может дать кадр, который ни один
        // клиент не соберёт. Отказ здесь, а не в нарезке, чтобы назвать число
        // выходов, как требует 11.3.
        if 7 + payload.len() + 1 + 76 * signers.len() > PANEL_REFUSE {
            return Err(CatalogError::TooLarge {
                payload_len: payload.len(),
                exits: self.exits.len(),
            });
        }
        let payload = match bucket {
            Some(r) => pad_to_bucket(self.to_map(), signers.len(), r, PANEL_REFUSE)?,
            None => payload,
        };
        let frame = frame::build(DocType::Catalog, &payload, signers)?;
        let chunks = chunk_frames_with(&frame, &self.pid, self.ver, self.iat, signers, bucket)?;
        Ok(SignedCatalog {
            chash: frame::frame_digest(&frame),
            chunks,
            payload_len: payload.len(),
            frame,
        })
    }
}

/// Часть каталога, `doc_type = 0x04` (`03-WIRE.md` 8.4): срез полного КАДРА
/// каталога. Конверт несёт `ver` и `iat` самого каталога, потому что часть
/// не имеет собственной истории версий: её тождество это `cid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogChunk {
    pub pid: [u8; 8],
    pub ver: u64,
    pub iat: u64,
    /// `chash[0..10]` каталога, те же байты, что `cat_id` до base32.
    pub cid: [u8; 10],
    /// Индекс с нуля.
    pub index: u64,
    /// Общее число частей; равно `cn` директивы.
    pub total: u64,
    /// Длина собранного кадра каталога.
    pub total_len: u64,
    /// Срез кадра `[index * 2816, min((index + 1) * 2816, total_len))`.
    pub data: Vec<u8>,
}

impl CatalogChunk {
    fn to_map(&self) -> BTreeMap<u64, Value> {
        let mut m = envelope(&self.pid, self.ver, self.iat, LIFETIME_CATALOG);
        m.insert(10, Value::Bytes(self.cid.to_vec()));
        m.insert(11, Value::Uint(self.index));
        m.insert(12, Value::Uint(self.total));
        m.insert(13, Value::Uint(self.total_len));
        m.insert(14, Value::Bytes(self.data.clone()));
        m
    }

    pub fn to_value(&self) -> Value {
        Value::Map(self.to_map())
    }

    pub fn encode(&self) -> Result<Vec<u8>, cbor::LimitError> {
        let v = self.to_value();
        cbor::check(&v)?;
        Ok(cbor::encode(&v))
    }

    /// Payload части с набивкой под `CHUNK_RESP_MAX`: полная часть в 2816
    /// байт даёт кадр до 2959, то есть 3072 на сетке, и `r` зажимается до 2.
    pub fn encode_padded(&self, nsigs: usize, bucket: u32) -> Result<Vec<u8>, PadError> {
        pad_to_bucket(self.to_map(), nsigs, bucket, CHUNK_RESP_MAX)
    }
}

/// Режет кадр каталога на payload частей. Каталог из одной части идёт тем
/// же путём: ветки без нарезки нет ни здесь, ни у клиента.
pub fn chunk_payloads(
    catalog_frame: &[u8],
    pid: &[u8; 8],
    ver: u64,
    iat: u64,
) -> Result<Vec<CatalogChunk>, CatalogError> {
    let tl = catalog_frame.len();
    if tl == 0 {
        return Err(CatalogError::Invalid("пустой кадр каталога".into()));
    }
    if tl > MAX_PAYLOAD_LEN {
        return Err(CatalogError::FrameTooLong(tl));
    }
    let n = tl.div_ceil(CHUNK_PAYLOAD_MAX);
    if n > MAX_CHUNKS {
        return Err(CatalogError::TooManyChunks(n));
    }
    let digest = frame::frame_digest(catalog_frame);
    let mut cid = [0u8; 10];
    cid.copy_from_slice(&digest[..10]);
    Ok(catalog_frame
        .chunks(CHUNK_PAYLOAD_MAX)
        .enumerate()
        .map(|(i, slice)| CatalogChunk {
            pid: *pid,
            ver,
            iat,
            cid,
            index: i as u64,
            total: n as u64,
            total_len: tl as u64,
            data: slice.to_vec(),
        })
        .collect())
}

/// Режет кадр каталога и подписывает каждую часть отдельно, чтобы подделка
/// ловилась до сборки, а собранный кадр проверялся ещё раз целиком. Без
/// набивки: путь генератора корпуса.
pub fn chunk_frames(
    catalog_frame: &[u8],
    pid: &[u8; 8],
    ver: u64,
    iat: u64,
    signers: &[SigningKey],
) -> Result<Vec<Vec<u8>>, CatalogError> {
    chunk_frames_with(catalog_frame, pid, ver, iat, signers, None)
}

/// То же с набивкой каждой части под `CHUNK_RESP_MAX` одной корзиной `r`.
pub fn chunk_frames_padded(
    catalog_frame: &[u8],
    pid: &[u8; 8],
    ver: u64,
    iat: u64,
    signers: &[SigningKey],
    bucket: u32,
) -> Result<Vec<Vec<u8>>, CatalogError> {
    chunk_frames_with(catalog_frame, pid, ver, iat, signers, Some(bucket))
}

fn chunk_frames_with(
    catalog_frame: &[u8],
    pid: &[u8; 8],
    ver: u64,
    iat: u64,
    signers: &[SigningKey],
    bucket: Option<u32>,
) -> Result<Vec<Vec<u8>>, CatalogError> {
    chunk_payloads(catalog_frame, pid, ver, iat)?
        .iter()
        .map(|c| {
            let payload = match bucket {
                Some(r) => c.encode_padded(signers.len(), r)?,
                None => c.encode()?,
            };
            Ok(frame::build(DocType::CatalogChunk, &payload, signers)?)
        })
        .collect()
}

/// `cat_id` (`03-WIRE.md` раздел 4): base32 Crockford от `chash[0..10]`,
/// ровно 16 символов без набивки.
pub fn cat_id(chash: &[u8; 32]) -> String {
    base32_crockford(&chash[..10])
}

/// base32 Crockford (`03-WIRE.md` 4.1): поток бит от старшего, по 5 бит на
/// символ, хвост дополняется нулями, знака `=` нет никогда.
pub fn base32_crockford(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Общий конверт, ключи 1..5. Повторяет `docs::envelope`, который приватен
/// для своего модуля.
fn envelope(pid: &[u8; 8], ver: u64, iat: u64, lifetime: u64) -> BTreeMap<u64, Value> {
    let mut m = BTreeMap::new();
    m.insert(1, Value::Uint(SPEC_VERSION));
    m.insert(2, Value::Bytes(pid.to_vec()));
    m.insert(3, Value::Uint(ver));
    m.insert(4, Value::Uint(iat));
    m.insert(5, Value::Uint(iat + lifetime));
    m
}

/// Запись зеркала в том же виде, что в bootstrap-блобе (`docs::Mirror`
/// кодирует себя приватно, поэтому кодирование повторено здесь; оба сверены с
/// корпусом, а расхождение поймает гейт).
fn mirror_value(m: &Mirror) -> Value {
    let mut v = BTreeMap::new();
    v.insert(1, Value::Text(m.host.clone()));
    v.insert(2, Value::Text(m.sni.clone()));
    v.insert(
        3,
        Value::Array(m.pins.iter().map(|p| Value::Bytes(p.to_vec())).collect()),
    );
    v.insert(4, Value::Uint(m.asn));
    v.insert(5, Value::Text(m.country.clone()));
    if let Some(w) = m.weight {
        v.insert(6, Value::Uint(w));
    }
    if !m.ips.is_empty() {
        v.insert(
            7,
            Value::Array(m.ips.iter().map(|i| Value::Text(i.clone())).collect()),
        );
    }
    Value::Map(v)
}

/// Запись DoH, см. [`mirror_value`].
fn doh_value(d: &DohEntry) -> Value {
    Value::map([
        (1, Value::Text(d.host.clone())),
        (2, Value::Text(d.path.clone())),
        (
            3,
            Value::Array(d.ips.iter().map(|i| Value::Text(i.clone())).collect()),
        ),
        (
            4,
            Value::Array(d.pins.iter().map(|p| Value::Bytes(p.to_vec())).collect()),
        ),
    ])
}

fn validate_mirror(m: &Mirror) -> Result<(), CatalogError> {
    let ctx = |reason: &str| CatalogError::Invalid(format!("mir {:?}: {reason}", m.host));
    if !is_hostname(&m.host) {
        return Err(ctx("h не hostname"));
    }
    if !is_hostname(&m.sni) {
        return Err(ctx("sni не hostname"));
    }
    if m.pins.is_empty() || m.pins.len() > 4 {
        return Err(ctx("pin вне 1..4"));
    }
    if m.asn == 0 || m.asn >= 1 << 32 {
        return Err(ctx("asn вне 1..2^32"));
    }
    if !is_country(&m.country) {
        return Err(ctx("cc не две заглавные буквы"));
    }
    if let Some(w) = m.weight
        && !(1..=100).contains(&w)
    {
        return Err(ctx("w вне 1..100"));
    }
    if m.ips.len() > 4 {
        return Err(ctx("ip длиннее 4"));
    }
    if !m.ips.iter().all(|ip| is_ip_literal(ip)) {
        return Err(ctx("ip не канонический IP-литерал"));
    }
    Ok(())
}

fn validate_doh(d: &DohEntry) -> Result<(), CatalogError> {
    let ctx = |reason: &str| CatalogError::Invalid(format!("doh {:?}: {reason}", d.host));
    if !is_hostname(&d.host) {
        return Err(ctx("h не hostname"));
    }
    if !is_path(&d.path) || d.path.len() > 64 {
        return Err(ctx("p не path-only длиной <= 64"));
    }
    if d.ips.is_empty() || d.ips.len() > 4 {
        return Err(ctx("ip вне 1..4"));
    }
    if !d.ips.iter().all(|ip| is_ip_literal(ip)) {
        return Err(ctx("ip не канонический IP-литерал"));
    }
    if d.pins.is_empty() || d.pins.len() > 4 {
        return Err(ctx("pin вне 1..4"));
    }
    Ok(())
}

/// Идентификатор узла: 1..24 символов `[0-9A-Za-z_-]`.
fn is_node_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 24
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_country(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
}

/// Hostname по `03-WIRE.md` 14.1. Верхний регистр отвергается, а не
/// нормализуется: два написания одного хоста дали бы два `chash`.
fn is_hostname(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 || !s.is_ascii() {
        return false;
    }
    s.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// IP-литерал в единственном каноническом написании: dotted-quad IPv4 или
/// IPv6 по RFC 5952 в нижнем регистре, что и даёт `Display` стандартной
/// библиотеки.
fn is_ip_literal(s: &str) -> bool {
    if let Ok(v4) = s.parse::<Ipv4Addr>() {
        return v4.to_string() == s;
    }
    if let Ok(v6) = s.parse::<Ipv6Addr>() {
        return v6.to_string() == s;
    }
    false
}

/// Путь без хоста по `03-WIRE.md` 14.2.
fn is_path(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 || !s.is_ascii() {
        return false;
    }
    if !s.starts_with('/') || s.starts_with("//") {
        return false;
    }
    if s.contains("://") || s.contains('@') || s.contains('\\') {
        return false;
    }
    if s.bytes()
        .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return false;
    }
    if s.split('/').any(|seg| seg == "..") {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    if lower.contains("%2f") {
        return false;
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                return false;
            }
            i += 3;
            continue;
        }
        let ok = b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=/:@?".contains(&b);
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit(id: &str) -> Node {
        let mut n = Node::new(
            id,
            "\u{1F1E9}\u{1F1EA} Stealth",
            "DE",
            "de1.exa-nodes.net",
            443,
            Protocol::Vless,
            Network::Tcp,
            Security::Reality,
        );
        n.sni = Some("www.microsoft.com".into());
        n.public_key = Some([7u8; 32]);
        n.short_id = Some("6ba85179".into());
        n.fingerprint = Some(Fingerprint::Chrome);
        n.flow = Some(Flow::Vision);
        n.insecure = Some(false);
        n
    }

    fn catalog(exits: Vec<Node>) -> Catalog {
        Catalog {
            pid: [1u8; 8],
            ver: 1,
            iat: 1_788_307_200,
            tier: 1,
            exits,
            relays: Vec::new(),
            routes: Vec::new(),
            cap: cap::NODE_MATERIAL | cap::SEALED_DIRECTIVES,
            mirrors: Vec::new(),
            doh: Vec::new(),
            rulesets: Vec::new(),
            geo: Vec::new(),
            ttl: 7200,
            jitter: 20,
            thresholds: Thresholds::default(),
            pad_buckets: [0, 3],
            ladder: None,
            pins: Vec::new(),
            hpke: None,
        }
    }

    #[test]
    fn content_digest_ignores_the_envelope() {
        let a = catalog(vec![exit("n1i1")]);
        let mut b = a.clone();
        b.ver += 5;
        b.iat += 86_400;
        b.pid = [9u8; 8];
        assert_eq!(a.content_digest(), b.content_digest());
    }

    #[test]
    fn content_digest_ignores_row_order_and_sees_content() {
        let a = catalog(vec![exit("n1i1"), exit("n2i1")]);
        let b = catalog(vec![exit("n2i1"), exit("n1i1")]);
        assert_eq!(a.content_digest(), b.content_digest());
        assert_eq!(a.encode().unwrap(), b.encode().unwrap());

        let mut c = a.clone();
        c.cap |= cap::SETTINGS_WRITE;
        assert_ne!(a.content_digest(), c.content_digest());
        let mut d = a.clone();
        d.exits[0].port = 8443;
        assert_ne!(a.content_digest(), d.content_digest());
    }

    #[test]
    fn empty_exit_list_is_refused() {
        let c = catalog(Vec::new());
        assert!(matches!(c.encode(), Err(CatalogError::Invalid(_))));
    }

    #[test]
    fn relay_reference_must_resolve() {
        let mut e = exit("n1i1");
        e.relay = Some("r9i1".into());
        let c = catalog(vec![e]);
        assert!(matches!(c.encode(), Err(CatalogError::Invalid(_))));
    }

    #[test]
    fn doh_host_must_be_a_mirror() {
        let mut c = catalog(vec![exit("n1i1")]);
        c.doh.push(DohEntry {
            host: "doh.example.net".into(),
            path: "/dns-query".into(),
            ips: vec!["198.51.100.7".into()],
            pins: vec![[1u8; 32]],
        });
        assert!(matches!(c.encode(), Err(CatalogError::Invalid(_))));
    }

    #[test]
    fn chunking_covers_the_whole_frame_in_order() {
        let frame: Vec<u8> = (0..7000u32).map(|i| i as u8).collect();
        let chunks = chunk_payloads(&frame, &[1u8; 8], 3, 100).unwrap();
        assert_eq!(chunks.len(), 3);
        assert!(
            chunks[..2]
                .iter()
                .all(|c| c.data.len() == CHUNK_PAYLOAD_MAX)
        );
        assert_eq!(chunks[2].data.len(), 7000 - 2 * CHUNK_PAYLOAD_MAX);
        let joined: Vec<u8> = chunks.iter().flat_map(|c| c.data.clone()).collect();
        assert_eq!(joined, frame);
        let digest = frame::frame_digest(&frame);
        assert!(
            chunks
                .iter()
                .all(|c| c.cid[..] == digest[..10] && c.total == 3 && c.total_len == 7000)
        );
    }

    #[test]
    fn signed_chunks_stay_under_the_response_ceiling() {
        let exits: Vec<Node> = (0..60).map(|i| exit(&format!("n{i}i1"))).collect();
        let signed = catalog(exits)
            .sign(&[SigningKey::from_bytes(&[5u8; 32])])
            .unwrap();
        assert!(signed.chunks.len() > 1);
        // d + 59 байт payload плюс 84 байта кадра, потолок 11.3.
        assert!(
            signed
                .chunks
                .iter()
                .all(|c| c.len() <= CHUNK_PAYLOAD_MAX + 143)
        );
        assert_eq!(
            signed.chunk_count(),
            signed.frame.len().div_ceil(CHUNK_PAYLOAD_MAX)
        );
    }

    #[test]
    fn padded_signing_lands_every_frame_on_the_grid() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let exits: Vec<Node> = (0..60).map(|i| exit(&format!("n{i}i1"))).collect();
        let model = catalog(exits);
        for r in 0..=3u32 {
            let signed = model.sign_padded(std::slice::from_ref(&key), r).unwrap();
            assert_eq!(signed.frame.len() % 256, 0, "кадр каталога вне сетки");
            assert!(signed.chunks.len() > 1);
            for c in &signed.chunks {
                assert_eq!(c.len() % 256, 0, "кадр части вне сетки");
                assert!(c.len() <= CHUNK_RESP_MAX, "часть выше CHUNK_RESP_MAX");
            }
            // Полная часть занимает 3072 при r = 0 и упирается в 3584 при r >= 2.
            let full = signed.chunks[0].len();
            assert_eq!(full, (3072 + 256 * r as usize).min(CHUNK_RESP_MAX));
            // Та же корзина дважды даёт те же байты: подпись детерминирована.
            assert_eq!(
                signed,
                model.sign_padded(std::slice::from_ref(&key), r).unwrap()
            );
        }
        // Набитый и ненабитый кадры это разные каталоги, гейт идёт без набивки.
        assert_ne!(
            model.sign(std::slice::from_ref(&key)).unwrap().chash,
            model.sign_padded(&[key], 0).unwrap().chash
        );
    }

    #[test]
    fn a_payload_that_fits_but_whose_frame_does_not_is_refused() {
        // Payload под PANEL_REFUSE, кадр над пределом tl: 8.4 ограничивает
        // ДЛИНУ КАДРА, и клиент отверг бы каждую часть как E_PARSE_FIELD.
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let mut model = catalog(Vec::new());
        let mut i = 0;
        loop {
            model.exits.push(exit(&format!("n{i}i1")));
            i += 1;
            let len = model.encode().unwrap().len();
            // Запас в одну запись узла, чтобы следующая не перескочила предел.
            if len > PANEL_REFUSE - 84 - 300 {
                break;
            }
        }
        // Дотягиваем по одному байту именами прокси: одно имя даёт не больше
        // 48 байт, поэтому имена растут по кругу.
        let mut k = 0;
        loop {
            let len = model.encode().unwrap().len();
            if 7 + len + 1 + 76 > PANEL_REFUSE {
                break;
            }
            let idx = k % model.exits.len();
            if model.exits[idx].pn.len() < 64 {
                model.exits[idx].pn.push('x');
            }
            k += 1;
        }
        let payload_len = model.encode().unwrap().len();
        assert!(payload_len <= PANEL_REFUSE, "encode обязан пройти");
        assert!(matches!(
            model.sign(std::slice::from_ref(&key)),
            Err(CatalogError::TooLarge { exits, .. }) if exits == model.exits.len()
        ));
        assert!(matches!(
            model.sign_padded(std::slice::from_ref(&key), 0),
            Err(CatalogError::TooLarge { .. })
        ));
        // Низкоуровневая нарезка тоже отвергает такой кадр.
        let frame = vec![0u8; MAX_PAYLOAD_LEN + 1];
        assert!(matches!(
            chunk_payloads(&frame, &[1u8; 8], 1, 1),
            Err(CatalogError::FrameTooLong(_))
        ));
    }

    #[test]
    fn storage_digest_binds_the_signer_and_the_tenant() {
        let a = catalog(vec![exit("n1i1")]);
        let mut b = a.clone();
        b.ver += 5;
        b.iat += 86_400;
        assert_eq!(a.storage_digest(&[1u8; 12]), b.storage_digest(&[1u8; 12]));
        assert_ne!(a.storage_digest(&[1u8; 12]), a.storage_digest(&[2u8; 12]));
        let mut c = a.clone();
        c.pid = [9u8; 8];
        assert_ne!(a.storage_digest(&[1u8; 12]), c.storage_digest(&[1u8; 12]));
        assert_ne!(a.storage_digest(&[1u8; 12]), a.content_digest());
    }

    #[test]
    fn host_header_equal_to_sni_and_a_preset_outside_the_vocabulary_are_refused() {
        let mut e = exit("n1i1");
        e.host_header = e.sni.clone();
        assert!(matches!(
            catalog(vec![e]).encode(),
            Err(CatalogError::Invalid(_))
        ));
        let mut ok = exit("n1i1");
        ok.host_header = Some("cdn.example.net".into());
        catalog(vec![ok]).encode().unwrap();

        let mut c = catalog(vec![exit("n1i1")]);
        c.routes.push(Route {
            id: "full".into(),
            name: "Full".into(),
            rulesets: Vec::new(),
        });
        assert!(matches!(c.encode(), Err(CatalogError::Invalid(_))));
        c.routes[0].id = "ru-smart".into();
        c.encode().unwrap();
        let mut over = catalog(vec![exit("n1i1")]);
        over.ver = 1 << 32;
        assert!(matches!(over.encode(), Err(CatalogError::Invalid(_))));
    }

    #[test]
    fn crockford_matches_the_spec_shape() {
        assert_eq!(base32_crockford(&[0u8; 10]).len(), 16);
        assert_eq!(base32_crockford(&[0xff; 10]), "ZZZZZZZZZZZZZZZZ");
        // 16 бит это четыре символа: 00000 00001 00010 0 плюс 4 бита набивки.
        assert_eq!(base32_crockford(&[0x00, 0x44]), "0120");
    }

    #[test]
    fn hostname_and_path_rules() {
        assert!(is_hostname("m1.example-cdn.net"));
        assert!(!is_hostname("M1.example.net"));
        assert!(!is_hostname("-bad.example.net"));
        assert!(!is_hostname("example.net."));
        assert!(is_ip_literal("198.51.100.7"));
        assert!(!is_ip_literal("198.051.100.7"));
        assert!(is_ip_literal("2001:db8::1"));
        assert!(!is_ip_literal("2001:DB8::1"));
        assert!(is_path("/dns-query"));
        assert!(is_path("/rulesets/ru-direct.srs"));
        assert!(!is_path("//evil/path"));
        assert!(!is_path("/a/../b"));
        assert!(!is_path("/a%2Fb"));
        assert!(!is_path("https://x/y"));
    }
}

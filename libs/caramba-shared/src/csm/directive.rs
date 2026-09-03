//! Директива, `doc_type = 0x03`, и её запечатанная обёртка, `doc_type = 0x06`
//! (`03-WIRE.md` 8.3, 9, 12; `02-SPEC.md` 4.6, 5.3, 7).
//!
//! Директива это единственный документ, который панель подписывает НА КАЖДЫЙ
//! запрос: она отвечает на конкретный nonce конкретного устройства. Поэтому
//! всё, что клиент вправе отвергнуть при разборе, здесь отвергается ещё до
//! подписи: закрытые словари выражены типами, пределы полей и три предиката
//! согласованности `sel` и `pol` проверяются в `validate`. Панель не имеет права
//! выпустить директиву, которую ни один клиент не примет.
//!
//! Отказ в обслуживании тоже едет здесь. Отозванная или заблокированная
//! подписка получает подписанный `st` и `rc` внутри ответа 200, а не голый 403:
//! подписанное поле переживает кэш и зеркало, а строка статуса нет.
//!
//! Запечатывание HPKE (раздел 9) здесь только в части кадрирования: внешний
//! payload, `info`, `aad` и проверки размеров. Сама криптография вынесена за шов
//! [`Sealer`], потому что нужного ей AEAD в рабочем пространстве нет.

use std::collections::BTreeMap;

use ed25519_dalek::SigningKey;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::cbor::{self, Value};
use super::docs::{LIFETIME_DIRECTIVE, SPEC_VERSION};
use super::frame::{self, DocType, FrameError, MAGIC};
use super::pad::PadError;

// Арифметика корзин общая для всех типов документов и живёт в `pad`; здесь
// она переэкспортирована, потому что директива это её основной потребитель.
pub use super::pad::{PAD_UNIT, pad_to_bucket};

// ---------------------------------------------------------------- константы

/// Потолок внутренней директивы после паддинга. Выше него `ct` не влезает в
/// `MAX_BSTR_BYTES`, то есть у кадра нет пути доставки (`03-WIRE.md` 12.2).
pub const INNER_DIRECTIVE_MAX: usize = 2816;

/// Потолок ответа, он же потолок запечатанного кадра (`03-WIRE.md` 17).
pub const RESP_MAX: usize = 4096;

/// Идентификаторы набора HPKE (`03-WIRE.md` 9.1). Другие в v1 не существуют.
pub const HPKE_KEM_P256: u64 = 0x0010;
pub const HPKE_KDF_SHA256: u64 = 0x0001;
pub const HPKE_AEAD_CHACHA20POLY1305: u64 = 0x0003;

/// `info` запечатывания, 12 байт ASCII без NUL (`03-WIRE.md` 9.2).
pub const SEAL_INFO: &[u8; 12] = b"CSM1-seal-v1";

/// Длина `enc`: несжатая точка P-256 `0x04 || X || Y`.
pub const ENC_LEN: usize = 65;

/// Границы `ct` (`03-WIRE.md` 9.3): минимальный кадр 0x03 плюс тег, и
/// `MAX_BSTR_BYTES`.
pub const CT_MIN: usize = 242;
pub const CT_MAX: usize = cbor::MAX_BSTR_BYTES;

/// Пределы полей директивы (`03-WIRE.md` 8.3).
pub const TTL_MIN: u64 = 300;
pub const TTL_MAX: u64 = 86_400;
pub const EXPH_MAX: u64 = 2_592_000;
pub const CHUNKS_MAX: u64 = 64;
pub const TIER_MAX: u64 = 1023;
pub const TEXT_MAX: usize = 80;
pub const HINTS_MAX: usize = 4;
pub const NODE_ID_MAX: usize = 24;
pub const PRESET_ID_MAX: usize = 32;
pub const LOCATOR_LEN: usize = 24;

/// Алфавит base32 Crockford (`03-WIRE.md` 4.1).
pub const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

// ---------------------------------------------------------------- ошибки

/// Ошибка сборки директивы. Каждый вариант это документ, который клиент бы
/// отверг, поэтому панель отказывается его подписывать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveError {
    /// Поле вне предела или словаря спецификации.
    Field { field: &'static str, reason: String },
    /// Выход за пределы профиля CBOR.
    Limit(cbor::LimitError),
    /// Ошибка кадра.
    Frame(FrameError),
    /// Даже без паддинга кадр не помещается под потолок.
    Padding { frame_len: usize, ceiling: usize },
    /// Отказ реализации запечатывания.
    Seal(String),
}

impl std::fmt::Display for DirectiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DirectiveError::Field { field, reason } => {
                write!(f, "csm directive: поле {field}: {reason}")
            }
            DirectiveError::Limit(e) => write!(f, "csm directive: {e}"),
            DirectiveError::Frame(e) => write!(f, "csm directive: {e}"),
            DirectiveError::Padding { frame_len, ceiling } => write!(
                f,
                "csm directive: кадр в {frame_len} байт не помещается под потолок {ceiling}"
            ),
            DirectiveError::Seal(reason) => write!(f, "csm directive: запечатывание: {reason}"),
        }
    }
}

impl std::error::Error for DirectiveError {}

impl From<cbor::LimitError> for DirectiveError {
    fn from(e: cbor::LimitError) -> Self {
        DirectiveError::Limit(e)
    }
}

impl From<FrameError> for DirectiveError {
    fn from(e: FrameError) -> Self {
        DirectiveError::Frame(e)
    }
}

impl From<PadError> for DirectiveError {
    fn from(e: PadError) -> Self {
        match e {
            PadError::AlreadyPadded => field_err("pd", "паддинг уже присутствует"),
            PadError::Limit(l) => DirectiveError::Limit(l),
            PadError::Ceiling { frame_len, ceiling } => {
                DirectiveError::Padding { frame_len, ceiling }
            }
        }
    }
}

fn field_err(field: &'static str, reason: impl Into<String>) -> DirectiveError {
    DirectiveError::Field {
        field,
        reason: reason.into(),
    }
}

// ---------------------------------------------------------------- base32 Crockford

/// Кодирует байты в base32 Crockford без символов дополнения.
pub fn crockford_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() * 8).div_ceil(5));
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &byte in input {
        acc = (acc << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD_ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD_ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Декодирует base32 Crockford по правилам читателя из `03-WIRE.md` 4.1:
/// строчные буквы принимаются, `I`/`L` это 1, `O` это 0, дефисы пропускаются,
/// ненулевые хвостовые биты отвергаются, чтобы у байтов было ровно одно
/// принимаемое написание.
pub fn crockford_decode(s: &str) -> Result<Vec<u8>, DirectiveError> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8 + 1);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for ch in s.chars() {
        let v = match ch {
            '-' => continue,
            '0'..='9' => ch as u32 - '0' as u32,
            'I' | 'i' | 'L' | 'l' => 1,
            'O' | 'o' => 0,
            'A'..='Z' | 'a'..='z' => {
                let up = ch.to_ascii_uppercase() as u8;
                match CROCKFORD_ALPHABET.iter().position(|&c| c == up) {
                    Some(i) => i as u32,
                    None => {
                        return Err(field_err(
                            "base32",
                            format!("символ {ch:?} вне алфавита Crockford"),
                        ));
                    }
                }
            }
            _ => {
                return Err(field_err(
                    "base32",
                    format!("символ {ch:?} вне алфавита Crockford"),
                ));
            }
        };
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    // Хвост короче одного символа и нулевой: иначе строка это не кодирование
    // никакой последовательности байт.
    if bits >= 5 || (acc & ((1 << bits) - 1)) != 0 {
        return Err(field_err("base32", "ненулевые или лишние хвостовые биты"));
    }
    Ok(out)
}

/// Ровно столько символов, сколько даёт кодирование `N` байт: длина
/// проверяется до декодирования, чтобы неверный ввод отвергался без работы и
/// без выделения памяти под него.
fn decode_fixed<const N: usize>(field: &'static str, s: &str) -> Result<[u8; N], DirectiveError> {
    let expected = (N * 8).div_ceil(5);
    if s.len() != expected {
        return Err(field_err(
            field,
            format!("ожидалось {expected} символов, получено {}", s.len()),
        ));
    }
    let bytes = crockford_decode(s)?;
    bytes.try_into().map_err(|v: Vec<u8>| {
        field_err(
            field,
            format!("ожидалось {N} байт, декодировано {}", v.len()),
        )
    })
}

// ---------------------------------------------------------------- идентификаторы

/// Nonce клиента, 16 байт из `?n=`. Директива строится только вокруг него,
/// поэтому без nonce её невозможно собрать по типу, а не по договорённости.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nonce(pub [u8; 16]);

impl Nonce {
    /// Разбирает значение параметра `n`, 26 символов base32 Crockford.
    pub fn from_query(s: &str) -> Result<Self, DirectiveError> {
        decode_fixed("nonce", s).map(Nonce)
    }

    /// Форма для `?n=`, как её шлёт клиент.
    pub fn to_query(&self) -> String {
        crockford_encode(&self.0)
    }
}

/// Отпечаток устройства, `sha256(SPKI DER подписывающего ключа)[0..16]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceThumbprint(pub [u8; 16]);

impl DeviceThumbprint {
    /// Разбирает параметр `d`, 26 символов base32 Crockford.
    pub fn from_query(s: &str) -> Result<Self, DirectiveError> {
        decode_fixed("dtp", s).map(DeviceThumbprint)
    }

    /// Отпечаток из DER SubjectPublicKeyInfo подписывающего ключа устройства.
    pub fn of_spki(spki_der: &[u8]) -> Self {
        use sha2::Digest;
        let d = Sha256::digest(spki_der);
        let mut out = [0u8; 16];
        out.copy_from_slice(&d[..16]);
        DeviceThumbprint(out)
    }

    pub fn to_query(&self) -> String {
        crockford_encode(&self.0)
    }
}

/// Локатор подписки: 24 символа base32 Crockford, `HMAC` от секрета тенанта,
/// UUID подписки и поколения (`03-WIRE.md` 4). Не обратим, поэтому поиск идёт
/// по индексной колонке, а не по расшифровке.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Locator(String);

impl Locator {
    /// Выводит локатор. `subscription_uuid` это ровно те 36 байт текста, что
    /// хранит панель, в нижнем регистре с дефисами; `generation` это колонка
    /// подписки, а не эпоха панели: одна утечка чинится одним `UPDATE`.
    pub fn derive(secret: &[u8], subscription_uuid: &str, generation: u32) -> Self {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(secret).expect("HMAC принимает ключ любой длины");
        mac.update(b"csm1-loc");
        mac.update(&[0x00]);
        mac.update(subscription_uuid.as_bytes());
        mac.update(&generation.to_be_bytes());
        let tag = mac.finalize().into_bytes();
        Locator(crockford_encode(&tag[..15]))
    }

    /// Разбирает локатор из пути запроса и приводит к каноническому написанию,
    /// чтобы `ea3b...` и `EA3B...` искались по одному ключу.
    pub fn parse(s: &str) -> Result<Self, DirectiveError> {
        let raw: [u8; 15] = decode_fixed("loc", s)?;
        Ok(Locator(crockford_encode(&raw)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ---------------------------------------------------------------- перечисления

/// Статус подписки, `st` (`02-SPEC.md` 4.6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Status {
    PendingApproval = 1,
    Onboarding = 2,
    Active = 3,
    Expired = 4,
    Revoked = 5,
    Suspended = 6,
    QuotaExceeded = 7,
    DeviceLimit = 8,
}

impl Status {
    /// Разрешено ли клиенту подключаться с этим статусом. Это единственное,
    /// что статус решает; всё остальное это текст на экране.
    pub fn may_connect(self) -> bool {
        matches!(self, Status::Onboarding | Status::Active)
    }
}

/// Машинный код причины, `rc`. Диапазоны, а не перечисление: клиент обязан
/// принять незнакомый код и показать общий текст статуса, поэтому панель вправе
/// выпускать значения из зарезервированных промежутков без смены версии.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReasonCode(pub u64);

impl ReasonCode {
    pub const NONE: ReasonCode = ReasonCode(0);
    pub const AWAITING_APPROVAL: ReasonCode = ReasonCode(1001);
    pub const ACCOUNT_SUSPENDED: ReasonCode = ReasonCode(1002);
    pub const ACCOUNT_CLOSED: ReasonCode = ReasonCode(1003);
    pub const TERM_ENDED: ReasonCode = ReasonCode(2001);
    pub const PAYMENT_FAILED: ReasonCode = ReasonCode(2002);
    pub const TRIAL_ENDED: ReasonCode = ReasonCode(2003);
    pub const TRAFFIC_QUOTA_EXHAUSTED: ReasonCode = ReasonCode(3001);
    pub const ONBOARDING_GRANT_EXHAUSTED: ReasonCode = ReasonCode(3002);
    pub const DAILY_ALLOWANCE_EXHAUSTED: ReasonCode = ReasonCode(3003);
    pub const DEVICE_LIMIT_REACHED: ReasonCode = ReasonCode(4001);
    pub const DEVICE_REVOKED_BY_USER: ReasonCode = ReasonCode(4002);
    pub const DEVICE_REVOKED_BY_OPERATOR: ReasonCode = ReasonCode(4003);
    pub const PLAN_WITHDRAWN: ReasonCode = ReasonCode(5001);
    pub const FLEET_UNAVAILABLE: ReasonCode = ReasonCode(5002);
}

/// Битовое поле возможностей, `cap` (`03-WIRE.md` 5.1). На проводе это
/// `bstr(4)` big-endian; биты 12..31 зарезервированы и подписант обязан
/// выпускать их нулями.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities(u32);

impl Capabilities {
    pub const NODE_MATERIAL: u32 = 1 << 0;
    pub const SEALED_DIRECTIVES: u32 = 1 << 1;
    pub const RELAY_CHAINING: u32 = 1 << 2;
    pub const SETTINGS_WRITE: u32 = 1 << 3;
    pub const MIRROR_POOL: u32 = 1 << 4;
    pub const DOH: u32 = 1 << 5;
    pub const RESOURCE_HASHES: u32 = 1 << 6;
    pub const DEPRECATION_CHANNEL: u32 = 1 << 7;
    pub const ONBOARDING_GRANT: u32 = 1 << 8;
    pub const DEVICE_ENROLLMENT: u32 = 1 << 9;
    pub const VARIANT_FORWARDED: u32 = 1 << 10;
    pub const PORT_HOPPING: u32 = 1 << 11;

    /// Маска определённых в v1 битов.
    pub const DEFINED: u32 = (1 << 12) - 1;

    /// Принимает только определённые биты: зарезервированный бит в подписанном
    /// документе это обещание, которого клиент не поймёт.
    pub fn from_bits(bits: u32) -> Result<Self, DirectiveError> {
        if bits & !Self::DEFINED != 0 {
            return Err(field_err(
                "cap",
                format!("зарезервированные биты выставлены: {bits:#010x}"),
            ));
        }
        Ok(Capabilities(bits))
    }

    pub fn from_be_bytes(b: [u8; 4]) -> Result<Self, DirectiveError> {
        Self::from_bits(u32::from_be_bytes(b))
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    pub fn to_be_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    pub fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
    }
}

/// Происхождение значения настройки, `src` (`01-DECISION.md` B2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Provenance {
    User = 1,
    Operator = 2,
    Default = 3,
}

/// Протокол узла в перечислении `pr` (`03-WIRE.md` 5). Для `sel.proto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum NodeProtocol {
    Vless = 1,
    Vmess = 2,
    Trojan = 3,
    Hysteria2 = 4,
    Tuic = 5,
    Shadowsocks = 6,
    Naive = 7,
    Wireguard = 8,
}

/// Словарь `pol[1]`, строки `CorePolicy.protocol` (`02-SPEC.md` 7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyProtocol {
    Auto,
    AmneziaWg,
    VlessReality,
    Vless,
    Hysteria2,
    Tuic,
    Shadowsocks,
}

impl PolicyProtocol {
    /// Строка на проводе. Пустая строка запрещена: `auto` едет буквально.
    pub fn wire(self) -> &'static str {
        match self {
            PolicyProtocol::Auto => "auto",
            PolicyProtocol::AmneziaWg => "AmneziaWG",
            PolicyProtocol::VlessReality => "VLESS-Reality",
            PolicyProtocol::Vless => "VLESS",
            PolicyProtocol::Hysteria2 => "Hysteria2",
            PolicyProtocol::Tuic => "TUIC",
            PolicyProtocol::Shadowsocks => "Shadowsocks",
        }
    }

    /// `PROTO_WIRE` (`02-SPEC.md` 7.4): проекция на `sel.proto`. Только в эту
    /// сторону: `VLESS` и `VLESS-Reality` схлопываются в одно значение.
    pub fn proto_wire(self) -> Option<NodeProtocol> {
        match self {
            PolicyProtocol::Auto => None,
            PolicyProtocol::AmneziaWg => Some(NodeProtocol::Wireguard),
            PolicyProtocol::VlessReality | PolicyProtocol::Vless => Some(NodeProtocol::Vless),
            PolicyProtocol::Hysteria2 => Some(NodeProtocol::Hysteria2),
            PolicyProtocol::Tuic => Some(NodeProtocol::Tuic),
            PolicyProtocol::Shadowsocks => Some(NodeProtocol::Shadowsocks),
        }
    }
}

/// Словарь пресетов маршрутизации (`02-SPEC.md` 7.3): идентификаторы ядра,
/// девять плюс пустая строка. UI-идентификатор `full` на провод не попадает.
pub const PRESET_VOCABULARY: &[&str] = &[
    "",
    "ru-smart",
    "ru-full",
    "telegram-only",
    "ir-smart",
    "by-smart",
    "cn-smart",
    "streaming",
    "adblock",
    "global",
];

/// Выбор релея пользователем, `pol[3]`: три состояния, а не два. Пустая
/// строка это «не выбирал», и её нельзя схлопывать на «без релея», иначе
/// цепочка релеев исчезает у всех, кто не трогал переключатель.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayChoice {
    Country([u8; 2]),
    NoRelay,
    Unset,
}

impl RelayChoice {
    fn wire(self) -> String {
        match self {
            RelayChoice::Country(cc) => String::from_utf8_lossy(&cc).into_owned(),
            RelayChoice::NoRelay => "--".into(),
            RelayChoice::Unset => String::new(),
        }
    }
}

/// Разрешённый релей, `sel.rcc`: всегда конкретное решение, страна или `--`.
/// Пустого состояния здесь нет по типу: панель обязана разрешить выбор сама.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayResolution {
    Country([u8; 2]),
    NoRelay,
}

impl RelayResolution {
    fn wire(self) -> String {
        match self {
            RelayResolution::Country(cc) => String::from_utf8_lossy(&cc).into_owned(),
            RelayResolution::NoRelay => "--".into(),
        }
    }
}

/// Сетевой стек, `pol[4]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    Gvisor,
    System,
    Mixed,
}

impl Stack {
    fn wire(self) -> &'static str {
        match self {
            Stack::Gvisor => "gvisor",
            Stack::System => "system",
            Stack::Mixed => "mixed",
        }
    }
}

/// Режим раздельного туннелирования, `pol[11]`. Список приложений сюда не
/// попадает никогда: у него нет ключа и его нельзя выразить этим типом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitMode {
    Off,
    Bypass,
    Allow,
}

impl SplitMode {
    fn wire(self) -> &'static str {
        match self {
            SplitMode::Off => "off",
            SplitMode::Bypass => "bypass",
            SplitMode::Allow => "allow",
        }
    }
}

/// Вид подсказки интерфейса, `ui.k`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum HintKind {
    Notice = 1,
    Warning = 2,
    Maintenance = 3,
    RenewalDue = 4,
    CapabilityUnavailable = 5,
}

// ---------------------------------------------------------------- составные поля

/// Авторитетный выбор, `sel` (`03-WIRE.md` 8.3). `rcc` и `nid` обязательны:
/// это те два поля, ради которых `sel` существует, они убирают зависимость
/// выдачи конфига от GeoIP по видимому адресу запроса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// `id` записи узла-выхода в каталоге.
    pub exit: Option<String>,
    /// `id` записи релея.
    pub relay: Option<String>,
    /// `id` пресета маршрутизации.
    pub preset: Option<String>,
    /// Индекс варианта соединения, 0 означает «нет» и не выпускается.
    pub variant: u8,
    /// Принудительный протокол; `None` это «авто».
    pub proto: Option<NodeProtocol>,
    /// Разрешённая страна релея.
    pub rcc: RelayResolution,
    /// Числовой `node_id` для легаси-запроса конфига.
    pub nid: u64,
}

/// Значение настройки с происхождением.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting<T> {
    pub value: T,
    pub src: Provenance,
}

impl<T> Setting<T> {
    pub fn new(value: T, src: Provenance) -> Self {
        Setting { value, src }
    }
}

/// Эхо настроек с происхождением, `pol` (`02-SPEC.md` 7.2). Поля это
/// закрытый реестр: настройка, которой здесь нет, не синхронизируется.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Policy {
    pub protocol: Option<Setting<PolicyProtocol>>,
    pub preset: Option<Setting<String>>,
    pub relay: Option<Setting<RelayChoice>>,
    pub stack: Option<Setting<Stack>>,
    pub mtu: Option<Setting<u64>>,
    pub ipv6: Option<Setting<bool>>,
    pub fake_ip: Option<Setting<bool>>,
    pub kill_switch: Option<Setting<bool>>,
    pub dns_nameservers: Option<Setting<Vec<String>>>,
    pub dns_fallback: Option<Setting<Vec<String>>>,
    pub split_mode: Option<Setting<SplitMode>>,
}

impl Policy {
    fn is_empty(&self) -> bool {
        self.protocol.is_none()
            && self.preset.is_none()
            && self.relay.is_none()
            && self.stack.is_none()
            && self.mtu.is_none()
            && self.ipv6.is_none()
            && self.fake_ip.is_none()
            && self.kill_switch.is_none()
            && self.dns_nameservers.is_none()
            && self.dns_fallback.is_none()
            && self.split_mode.is_none()
    }
}

/// Подсказка интерфейса: инертный текст без ссылок, только на показ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub kind: HintKind,
    pub text: String,
}

/// Подписанные счётчики трафика, `traf`: замена заголовка
/// `Subscription-Userinfo`, которая переживает кэш и зеркало.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Traffic {
    pub up: u64,
    pub down: u64,
    /// Лимит в байтах, 0 означает «без ограничения».
    pub total: u64,
    /// Истечение подписки, Unix-секунды.
    pub expires: u64,
}

// ---------------------------------------------------------------- директива

/// Директива, `doc_type = 0x03`. Подписывается онлайн-ключом, живёт час,
/// привязана к nonce и устройству.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub pid: [u8; 8],
    /// Счётчик на локатор, выделяется в транзакции подписи (`02-SPEC.md` 4.7).
    pub ver: u64,
    pub iat: u64,
    pub nonce: Nonce,
    pub dtp: DeviceThumbprint,
    pub status: Status,
    pub reason: ReasonCode,
    /// `chash` каталога, к которому привязана директива.
    pub catalog: [u8; 32],
    /// Число частей, в которых отдаётся каталог.
    pub chunks: u64,
    pub tier: u64,
    pub cap: Capabilities,
    pub selection: Option<Selection>,
    pub policy: Option<Policy>,
    pub announce: Option<String>,
    pub support: Option<String>,
    pub hints: Vec<Hint>,
    /// Период обновления директивы, секунды.
    pub ttl: u64,
    /// Окно офлайн-льготы; `None` означает клиентский дефолт 604800.
    pub grace: Option<u64>,
    pub locator: Locator,
    pub traffic: Option<Traffic>,
}

fn check_text(field: &'static str, s: &str, max: usize) -> Result<(), DirectiveError> {
    if s.len() > max {
        return Err(field_err(
            field,
            format!("{} байт при пределе {max}", s.len()),
        ));
    }
    Ok(())
}

fn check_node_id(field: &'static str, id: &str) -> Result<(), DirectiveError> {
    if id.is_empty() || id.len() > NODE_ID_MAX {
        return Err(field_err(field, "длина вне 1..24"));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(field_err(field, "символ вне [0-9A-Za-z_-]"));
    }
    if id == "default" {
        return Err(field_err(field, "литерал default зарезервирован"));
    }
    Ok(())
}

fn check_country(field: &'static str, cc: &[u8; 2]) -> Result<(), DirectiveError> {
    if !cc.iter().all(u8::is_ascii_uppercase) {
        return Err(field_err(field, "код страны это две заглавные буквы ASCII"));
    }
    Ok(())
}

fn check_preset(field: &'static str, id: &str) -> Result<(), DirectiveError> {
    if !PRESET_VOCABULARY.contains(&id) {
        return Err(field_err(field, format!("пресет {id:?} вне словаря ядра")));
    }
    Ok(())
}

fn check_resolvers(field: &'static str, list: &[String]) -> Result<(), DirectiveError> {
    if list.len() > 8 {
        return Err(field_err(field, "больше 8 резолверов"));
    }
    for url in list {
        if url.is_empty() || url.len() > 128 {
            return Err(field_err(field, "длина адреса резолвера вне 1..128"));
        }
        // INV-8 распространяется на DNS: только DoH и DoT.
        if !(url.starts_with("https://") || url.starts_with("tls://")) {
            return Err(field_err(
                field,
                format!("резолвер {url:?} не https:// и не tls://"),
            ));
        }
    }
    Ok(())
}

impl Selection {
    fn validate(&self) -> Result<(), DirectiveError> {
        if let Some(e) = &self.exit {
            check_node_id("sel.exit", e)?;
        }
        if let Some(r) = &self.relay {
            check_node_id("sel.relay", r)?;
        }
        if let Some(p) = &self.preset {
            check_text("sel.preset", p, PRESET_ID_MAX)?;
            check_preset("sel.preset", p)?;
        }
        if let RelayResolution::Country(cc) = &self.rcc {
            check_country("sel.rcc", cc)?;
        }
        if self.nid == 0 {
            return Err(field_err("sel.nid", "node_id 0 не именует узел"));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        if let Some(e) = &self.exit {
            m.insert(1, Value::Text(e.clone()));
        }
        if let Some(r) = &self.relay {
            m.insert(2, Value::Text(r.clone()));
        }
        if let Some(p) = &self.preset {
            m.insert(3, Value::Text(p.clone()));
        }
        if self.variant != 0 {
            m.insert(4, Value::Uint(u64::from(self.variant)));
        }
        if let Some(p) = self.proto {
            m.insert(5, Value::Uint(p as u64));
        }
        m.insert(6, Value::Text(self.rcc.wire()));
        m.insert(7, Value::Uint(self.nid));
        Value::Map(m)
    }
}

impl Policy {
    fn validate(&self) -> Result<(), DirectiveError> {
        if let Some(p) = &self.preset {
            check_text("pol.preset", &p.value, PRESET_ID_MAX)?;
            check_preset("pol.preset", &p.value)?;
        }
        if let Some(r) = &self.relay {
            match r.value {
                RelayChoice::Country(cc) => check_country("pol.relay", &cc)?,
                RelayChoice::NoRelay => {}
                // «Не выбирал» разрешает оператор, и провенанс обязан это
                // сказать, иначе клиент покажет чужое решение как выбор юзера.
                RelayChoice::Unset => {
                    if r.src != Provenance::Default {
                        return Err(field_err(
                            "pol.relay",
                            "пустой выбор релея обязан идти с src = default",
                        ));
                    }
                }
            }
        }
        if let Some(m) = &self.mtu
            && m.value != 0
            && !(576..=9000).contains(&m.value)
        {
            return Err(field_err("pol.mtu", "вне 0 или 576..9000"));
        }
        if let Some(d) = &self.dns_nameservers {
            check_resolvers("pol.dns.nameservers", &d.value)?;
        }
        if let Some(d) = &self.dns_fallback {
            check_resolvers("pol.dns.fallback", &d.value)?;
        }
        // Стек, IPv6 и fake-IP оператор писать не вправе: клиент такое значение
        // проигнорирует, значит подписывать его незачем.
        let operator_forbidden = [
            ("pol.stack", self.stack.as_ref().map(|s| s.src)),
            ("pol.ipv6", self.ipv6.as_ref().map(|s| s.src)),
            ("pol.fakeIp", self.fake_ip.as_ref().map(|s| s.src)),
        ];
        for (field, src) in operator_forbidden {
            if src == Some(Provenance::Operator) {
                return Err(field_err(field, "оператору запрещено писать эту настройку"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        fn pair<T>(s: &Setting<T>, v: Value) -> Value {
            Value::Array(vec![v, Value::Uint(s.src as u64)])
        }
        fn texts(list: &[String]) -> Value {
            Value::Array(list.iter().map(|s| Value::Text(s.clone())).collect())
        }
        let mut m = BTreeMap::new();
        if let Some(s) = &self.protocol {
            m.insert(1, pair(s, Value::Text(s.value.wire().into())));
        }
        if let Some(s) = &self.preset {
            m.insert(2, pair(s, Value::Text(s.value.clone())));
        }
        if let Some(s) = &self.relay {
            m.insert(3, pair(s, Value::Text(s.value.wire())));
        }
        if let Some(s) = &self.stack {
            m.insert(4, pair(s, Value::Text(s.value.wire().into())));
        }
        if let Some(s) = &self.mtu {
            m.insert(5, pair(s, Value::Uint(s.value)));
        }
        if let Some(s) = &self.ipv6 {
            m.insert(6, pair(s, Value::Bool(s.value)));
        }
        if let Some(s) = &self.fake_ip {
            m.insert(7, pair(s, Value::Bool(s.value)));
        }
        if let Some(s) = &self.kill_switch {
            m.insert(8, pair(s, Value::Bool(s.value)));
        }
        if let Some(s) = &self.dns_nameservers {
            m.insert(9, pair(s, texts(&s.value)));
        }
        if let Some(s) = &self.dns_fallback {
            m.insert(10, pair(s, texts(&s.value)));
        }
        if let Some(s) = &self.split_mode {
            m.insert(11, pair(s, Value::Text(s.value.wire().into())));
        }
        Value::Map(m)
    }
}

/// Три предиката согласованности `sel` и `pol` (`02-SPEC.md` 7.4). Клиент
/// проверяет их при разборе и отвергает документ целиком, поэтому подписант
/// проверяет их раньше.
fn check_agreement(sel: &Selection, pol: &Policy) -> Result<(), DirectiveError> {
    if let (Some(sp), Some(pp)) = (&sel.preset, &pol.preset)
        && sp != &pp.value
    {
        return Err(field_err("sel.preset", "расходится с pol.preset"));
    }
    if let (Some(proto), Some(pp)) = (sel.proto, &pol.protocol)
        && pp.value.proto_wire() != Some(proto)
    {
        return Err(field_err("sel.proto", "не равен PROTO_WIRE[pol.protocol]"));
    }
    if let Some(pr) = &pol.relay {
        let agrees = match (pr.value, sel.rcc) {
            (RelayChoice::Country(a), RelayResolution::Country(b)) => a.to_ascii_uppercase() == b,
            (RelayChoice::NoRelay, RelayResolution::NoRelay) => true,
            (RelayChoice::Unset, _) => true,
            _ => false,
        };
        if !agrees {
            return Err(field_err("sel.rcc", "расходится с pol.relay"));
        }
    }
    Ok(())
}

/// Общий конверт, ключи 1..5.
fn envelope(pid: &[u8; 8], ver: u64, iat: u64, lifetime: u64) -> BTreeMap<u64, Value> {
    let mut m = BTreeMap::new();
    m.insert(1, Value::Uint(SPEC_VERSION));
    m.insert(2, Value::Bytes(pid.to_vec()));
    m.insert(3, Value::Uint(ver));
    m.insert(4, Value::Uint(iat));
    m.insert(5, Value::Uint(iat + lifetime));
    m
}

impl Directive {
    /// Проверяет пределы полей и предикаты согласованности.
    pub fn validate(&self) -> Result<(), DirectiveError> {
        if self.ver >= 1 << 32 {
            return Err(field_err("ver", "не меньше 2^32"));
        }
        if self.chunks == 0 || self.chunks > CHUNKS_MAX {
            return Err(field_err("cn", "вне 1..64"));
        }
        if self.tier == 0 || self.tier > TIER_MAX {
            return Err(field_err("tier", "вне 1..1023"));
        }
        if !(TTL_MIN..=TTL_MAX).contains(&self.ttl) {
            return Err(field_err("ttl", "вне 300..86400"));
        }
        if let Some(g) = self.grace
            && g > EXPH_MAX
        {
            return Err(field_err("exph", "больше 2592000"));
        }
        if let Some(a) = &self.announce {
            check_text("ann", a, TEXT_MAX)?;
        }
        if let Some(s) = &self.support {
            check_text("sup", s, TEXT_MAX)?;
        }
        if self.hints.len() > HINTS_MAX {
            return Err(field_err("ui", "больше 4 подсказок"));
        }
        for h in &self.hints {
            check_text("ui.t", &h.text, TEXT_MAX)?;
        }
        if self.locator.as_str().len() != LOCATOR_LEN {
            return Err(field_err("loc", "не 24 символа"));
        }
        if let Some(sel) = &self.selection {
            sel.validate()?;
        }
        if let Some(pol) = &self.policy {
            pol.validate()?;
        }
        if let (Some(sel), Some(pol)) = (&self.selection, &self.policy) {
            check_agreement(sel, pol)?;
        }
        Ok(())
    }

    /// Собирает значение полезной нагрузки без паддинга. Порядок полей и
    /// правило «ноль и пусто не выпускаются» совпадают с генератором корпуса.
    pub fn to_value(&self) -> Value {
        Value::Map(self.to_map())
    }

    fn to_map(&self) -> BTreeMap<u64, Value> {
        let mut m = envelope(&self.pid, self.ver, self.iat, LIFETIME_DIRECTIVE);
        m.insert(10, Value::Bytes(self.nonce.0.to_vec()));
        m.insert(11, Value::Bytes(self.dtp.0.to_vec()));
        m.insert(12, Value::Uint(self.status as u64));
        if self.reason != ReasonCode::NONE {
            m.insert(13, Value::Uint(self.reason.0));
        }
        m.insert(14, Value::Bytes(self.catalog.to_vec()));
        m.insert(15, Value::Uint(self.chunks));
        m.insert(16, Value::Uint(self.tier));
        m.insert(17, Value::Bytes(self.cap.to_be_bytes().to_vec()));
        if let Some(sel) = &self.selection {
            m.insert(18, sel.to_value());
        }
        if let Some(pol) = &self.policy
            && !pol.is_empty()
        {
            m.insert(19, pol.to_value());
        }
        if let Some(a) = &self.announce
            && !a.is_empty()
        {
            m.insert(20, Value::Text(a.clone()));
        }
        if let Some(s) = &self.support
            && !s.is_empty()
        {
            m.insert(21, Value::Text(s.clone()));
        }
        if !self.hints.is_empty() {
            m.insert(
                22,
                Value::Array(
                    self.hints
                        .iter()
                        .map(|h| {
                            Value::map([
                                (1, Value::Uint(h.kind as u64)),
                                (2, Value::Text(h.text.clone())),
                            ])
                        })
                        .collect(),
                ),
            );
        }
        m.insert(23, Value::Uint(self.ttl));
        if let Some(g) = self.grace
            && g != 0
        {
            m.insert(24, Value::Uint(g));
        }
        m.insert(25, Value::Text(self.locator.as_str().to_owned()));
        if let Some(t) = &self.traffic {
            m.insert(
                26,
                Value::map([
                    (1, Value::Uint(t.up)),
                    (2, Value::Uint(t.down)),
                    (3, Value::Uint(t.total)),
                    (4, Value::Uint(t.expires)),
                ]),
            );
        }
        m
    }

    /// Кодирует полезную нагрузку без паддинга, проверив поля и профиль.
    pub fn encode(&self) -> Result<Vec<u8>, DirectiveError> {
        self.validate()?;
        let v = self.to_value();
        cbor::check(&v)?;
        Ok(cbor::encode(&v))
    }

    /// Кодирует полезную нагрузку с паддингом на сетку 256 байт: `bucket` это
    /// `r` из `[pb[0], pb[1]]`, вытянутый панелью на этот запрос, `nsigs` число
    /// подписей будущего кадра. Потолок внутренней директивы 2816, выше неё нет
    /// пути доставки.
    pub fn encode_padded(&self, nsigs: usize, bucket: u32) -> Result<Vec<u8>, DirectiveError> {
        self.validate()?;
        Ok(pad_to_bucket(
            self.to_map(),
            nsigs,
            bucket,
            INNER_DIRECTIVE_MAX,
        )?)
    }

    /// Собирает и подписывает внутренний кадр 0x03.
    pub fn sign(&self, signers: &[SigningKey], bucket: u32) -> Result<Vec<u8>, DirectiveError> {
        let payload = self.encode_padded(signers.len(), bucket)?;
        Ok(frame::build(DocType::Directive, &payload, signers)?)
    }

    /// Полный путь панели: подписать, запечатать к устройству, подписать
    /// обёртку. Это то, что уходит в тело ответа `/sub/m1/{loc}`.
    #[allow(clippy::too_many_arguments)]
    pub fn sign_sealed(
        &self,
        signers: &[SigningKey],
        inner_bucket: u32,
        recipient_pk: &[u8; ENC_LEN],
        recipient_generation: u64,
        sealer: &dyn Sealer,
        outer_bucket: u32,
    ) -> Result<Vec<u8>, DirectiveError> {
        let inner = self.sign(signers, inner_bucket)?;
        let sealed =
            SealedDirective::wrap(self, &inner, recipient_pk, recipient_generation, sealer)?;
        sealed.sign(signers, outer_bucket)
    }
}

// ---------------------------------------------------------------- запечатывание

/// Результат HPKE: инкапсулированный ключ и шифртекст с тегом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealOutput {
    pub enc: [u8; ENC_LEN],
    pub ct: Vec<u8>,
}

/// Шов для HPKE `Seal` в режиме base с набором `03-WIRE.md` 9.1:
/// `DHKEM(P-256, HKDF-SHA256)`, `HKDF-SHA256`, `ChaCha20Poly1305`.
///
/// Реализация вынесена за трейт, потому что в рабочем пространстве нет AEAD
/// ChaCha20-Poly1305, а тащить его сюда без решения оркестратора нельзя. Всё,
/// что вокруг (`info`, `aad`, поля обёртки, размеры), уже здесь и сверено с
/// корпусом.
pub trait Sealer {
    fn seal(
        &self,
        recipient_pk: &[u8; ENC_LEN],
        info: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<SealOutput, DirectiveError>;
}

/// `aad = "CSM1" || 0x06 || pid(8) || dtp(16) || u32be(ver)`, 33 байта
/// (`03-WIRE.md` 9.2). Получатель пересчитывает его сам из внешнего payload,
/// поэтому обе стороны считают его до всякой криптографии.
pub fn seal_aad(pid: &[u8; 8], dtp: &DeviceThumbprint, ver: u32) -> [u8; 33] {
    let mut out = [0u8; 33];
    out[..4].copy_from_slice(&MAGIC);
    out[4] = DocType::SealedDirective.as_u8();
    out[5..13].copy_from_slice(pid);
    out[13..29].copy_from_slice(&dtp.0);
    out[29..].copy_from_slice(&ver.to_be_bytes());
    out
}

/// Запечатанная директива, `doc_type = 0x06` (`03-WIRE.md` 9.3). Открытый
/// текст `ct` это ПОЛНЫЙ кадр 0x03 с магией и подписями, не голый payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedDirective {
    pub pid: [u8; 8],
    /// Равен `ver` внутренней директивы.
    pub ver: u64,
    pub iat: u64,
    pub dtp: DeviceThumbprint,
    pub enc: [u8; ENC_LEN],
    pub ct: Vec<u8>,
    /// Поколение ключа согласования УСТРОЙСТВА, не ключа панели из каталога.
    pub rkv: u64,
}

impl SealedDirective {
    /// Запечатывает подписанный внутренний кадр к ключу устройства. `pid`,
    /// `dtp` и `ver` берутся из самой директивы: внешний кадр не может
    /// адресоваться не тому устройству, для которого подписан внутренний.
    pub fn wrap(
        inner: &Directive,
        inner_frame: &[u8],
        recipient_pk: &[u8; ENC_LEN],
        recipient_generation: u64,
        sealer: &dyn Sealer,
    ) -> Result<Self, DirectiveError> {
        if inner_frame.len() < 5 || inner_frame[..4] != MAGIC || inner_frame[4] != 0x03 {
            return Err(DirectiveError::Seal(
                "открытый текст обязан быть полным кадром 0x03".into(),
            ));
        }
        if inner_frame.len() + 16 > CT_MAX {
            return Err(DirectiveError::Seal(format!(
                "внутренний кадр {} байт не помещается в ct",
                inner_frame.len()
            )));
        }
        if recipient_pk[0] != 0x04 {
            return Err(DirectiveError::Seal(
                "ключ получателя обязан быть несжатой точкой P-256".into(),
            ));
        }
        let aad = seal_aad(&inner.pid, &inner.dtp, inner.ver as u32);
        let out = sealer.seal(recipient_pk, SEAL_INFO, &aad, inner_frame)?;
        Ok(SealedDirective {
            pid: inner.pid,
            ver: inner.ver,
            iat: inner.iat,
            dtp: inner.dtp,
            enc: out.enc,
            ct: out.ct,
            rkv: recipient_generation,
        })
    }

    pub fn validate(&self) -> Result<(), DirectiveError> {
        if self.ver >= 1 << 32 {
            return Err(field_err("ver", "не меньше 2^32"));
        }
        if self.enc[0] != 0x04 {
            return Err(field_err("enc", "не несжатая точка P-256"));
        }
        if !(CT_MIN..=CT_MAX).contains(&self.ct.len()) {
            return Err(field_err("ct", "длина вне 242..3072"));
        }
        if self.rkv >= 1 << 16 {
            return Err(field_err("rkv", "не меньше 2^16"));
        }
        Ok(())
    }

    fn to_map(&self) -> BTreeMap<u64, Value> {
        let mut m = envelope(&self.pid, self.ver, self.iat, LIFETIME_DIRECTIVE);
        m.insert(10, Value::Bytes(self.dtp.0.to_vec()));
        m.insert(11, Value::Uint(HPKE_KEM_P256));
        m.insert(12, Value::Uint(HPKE_KDF_SHA256));
        m.insert(13, Value::Uint(HPKE_AEAD_CHACHA20POLY1305));
        m.insert(14, Value::Bytes(self.enc.to_vec()));
        m.insert(15, Value::Bytes(self.ct.clone()));
        m.insert(16, Value::Uint(self.rkv));
        m
    }

    pub fn to_value(&self) -> Value {
        Value::Map(self.to_map())
    }

    pub fn encode(&self) -> Result<Vec<u8>, DirectiveError> {
        self.validate()?;
        let v = self.to_value();
        cbor::check(&v)?;
        Ok(cbor::encode(&v))
    }

    /// Внешний паддинг под `thr.resp_max`: внутренний кадр уже зажат под 2816,
    /// здесь зажимается сам ответ.
    pub fn encode_padded(&self, nsigs: usize, bucket: u32) -> Result<Vec<u8>, DirectiveError> {
        self.validate()?;
        Ok(pad_to_bucket(self.to_map(), nsigs, bucket, RESP_MAX)?)
    }

    /// Собирает и подписывает внешний кадр 0x06.
    pub fn sign(&self, signers: &[SigningKey], bucket: u32) -> Result<Vec<u8>, DirectiveError> {
        let payload = self.encode_padded(signers.len(), bucket)?;
        Ok(frame::build(DocType::SealedDirective, &payload, signers)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crockford_matches_the_published_roundtrip() {
        // vectors.json derivations: 16 байт дают 26 символов с двумя битами
        // дополнения.
        let input: Vec<u8> = (0u8..16).collect();
        let s = crockford_encode(&input);
        assert_eq!(s, "000G40R40M30E209185GR38E1W");
        assert_eq!(crockford_decode(&s).unwrap(), input);
        assert_eq!(crockford_decode(&s.to_lowercase()).unwrap(), input);
    }

    #[test]
    fn crockford_reader_rules() {
        assert_eq!(crockford_decode("0-0-0G").unwrap(), vec![0, 1]);
        assert_eq!(
            crockford_decode("io").unwrap(),
            crockford_decode("10").unwrap()
        );
        assert_eq!(
            crockford_decode("Lo").unwrap(),
            crockford_decode("10").unwrap()
        );
        assert!(crockford_decode("U").is_err());
        // Ненулевые хвостовые биты: второе написание тех же байт запрещено.
        assert!(crockford_decode("000G40R40M30E209185GR38E1X").is_err());
        // Лишний символ, который не кодирует ни одного байта.
        assert!(crockford_decode("000").is_err());
    }

    #[test]
    fn nonce_and_thumbprint_take_exactly_sixteen_bytes() {
        let n = Nonce([7u8; 16]);
        assert_eq!(n.to_query().len(), 26);
        assert_eq!(Nonce::from_query(&n.to_query()).unwrap(), n);
        assert!(Nonce::from_query("000G40R40M30E209185GR38E1W0").is_err());
        assert!(DeviceThumbprint::from_query("49Q8M87PK6WP9QXG3T30").is_err());
    }

    #[test]
    fn locator_is_canonical_on_parse() {
        let l = Locator::derive(&[1u8; 32], "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8", 1);
        assert_eq!(l.as_str().len(), LOCATOR_LEN);
        assert_eq!(Locator::parse(&l.as_str().to_lowercase()).unwrap(), l);
        assert_ne!(
            Locator::derive(&[1u8; 32], "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8", 2),
            l
        );
        assert!(Locator::parse("49Q8M87PK6WP9QXG3T30").is_err());
    }

    #[test]
    fn capabilities_refuse_reserved_bits() {
        assert!(Capabilities::from_bits(1 << 12).is_err());
        let c = Capabilities::from_bits(Capabilities::NODE_MATERIAL | Capabilities::DOH).unwrap();
        assert_eq!(c.to_be_bytes(), [0, 0, 0, 0x21]);
        assert!(c.contains(Capabilities::DOH));
        assert!(!c.contains(Capabilities::RELAY_CHAINING));
    }

    #[test]
    fn seal_aad_is_thirty_three_bytes() {
        let aad = seal_aad(&[0x22; 8], &DeviceThumbprint([0x4f; 16]), 412);
        assert_eq!(&aad[..5], b"CSM1\x06");
        assert_eq!(&aad[29..], &[0, 0, 0x01, 0x9c]);
    }

    fn minimal() -> Directive {
        Directive {
            pid: [0x22; 8],
            ver: 1,
            iat: 1_788_307_200,
            nonce: Nonce([1; 16]),
            dtp: DeviceThumbprint([2; 16]),
            status: Status::Active,
            reason: ReasonCode::NONE,
            catalog: [3; 32],
            chunks: 1,
            tier: 1,
            cap: Capabilities::default(),
            selection: None,
            policy: None,
            announce: None,
            support: None,
            hints: Vec::new(),
            ttl: 7200,
            grace: None,
            locator: Locator::derive(&[0; 32], "u", 1),
            traffic: None,
        }
    }

    #[test]
    fn agreement_predicates_are_enforced_before_signing() {
        let mut d = minimal();
        d.selection = Some(Selection {
            exit: None,
            relay: None,
            preset: Some("ru-smart".into()),
            variant: 0,
            proto: Some(NodeProtocol::Vless),
            rcc: RelayResolution::Country(*b"NL"),
            nid: 5,
        });
        let mut pol = Policy {
            preset: Some(Setting::new("ru-full".into(), Provenance::User)),
            ..Policy::default()
        };
        d.policy = Some(pol.clone());
        assert!(matches!(
            d.encode(),
            Err(DirectiveError::Field {
                field: "sel.preset",
                ..
            })
        ));

        pol.preset = Some(Setting::new("ru-smart".into(), Provenance::User));
        pol.protocol = Some(Setting::new(PolicyProtocol::Hysteria2, Provenance::User));
        d.policy = Some(pol.clone());
        assert!(matches!(
            d.encode(),
            Err(DirectiveError::Field {
                field: "sel.proto",
                ..
            })
        ));

        pol.protocol = Some(Setting::new(PolicyProtocol::VlessReality, Provenance::User));
        pol.relay = Some(Setting::new(RelayChoice::NoRelay, Provenance::User));
        d.policy = Some(pol.clone());
        assert!(matches!(
            d.encode(),
            Err(DirectiveError::Field {
                field: "sel.rcc",
                ..
            })
        ));

        pol.relay = Some(Setting::new(RelayChoice::Unset, Provenance::Default));
        d.policy = Some(pol);
        d.encode().expect("согласованная директива подписывается");
    }

    #[test]
    fn vocabulary_and_limits_are_enforced() {
        let mut d = minimal();
        d.ttl = 299;
        assert!(d.encode().is_err());
        let mut d = minimal();
        d.tier = 1024;
        assert!(d.encode().is_err());
        let mut d = minimal();
        d.announce = Some("x".repeat(81));
        assert!(d.encode().is_err());
        let mut d = minimal();
        d.policy = Some(Policy {
            relay: Some(Setting::new(RelayChoice::Unset, Provenance::User)),
            ..Policy::default()
        });
        assert!(d.encode().is_err());
        let mut d = minimal();
        d.policy = Some(Policy {
            stack: Some(Setting::new(Stack::System, Provenance::Operator)),
            ..Policy::default()
        });
        assert!(d.encode().is_err());
        let mut d = minimal();
        d.policy = Some(Policy {
            dns_nameservers: Some(Setting::new(
                vec!["http://1.1.1.1/dns-query".into()],
                Provenance::User,
            )),
            ..Policy::default()
        });
        assert!(d.encode().is_err());
    }

    #[test]
    fn revocation_travels_as_a_signed_field() {
        let mut d = minimal();
        d.status = Status::Revoked;
        d.reason = ReasonCode::DEVICE_REVOKED_BY_OPERATOR;
        let bytes = d.encode().unwrap();
        // st = 5 под ключом 12, rc = 4003 под ключом 13, оба в подписи.
        assert!(bytes.windows(2).any(|w| w == [0x0c, 0x05]));
        assert!(bytes.windows(4).any(|w| w == [0x0d, 0x19, 0x0f, 0xa3]));
        assert!(!Status::Revoked.may_connect());
        assert!(Status::Onboarding.may_connect());
    }
}

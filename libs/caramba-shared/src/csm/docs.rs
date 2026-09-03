//! Сборка полезной нагрузки документов CSM/1 (`03-WIRE.md` раздел 8).
//!
//! Здесь только то, что панель ПОДПИСЫВАЕТ. Разбор и проверка живут у клиента
//! (Go в `libs/caramba-core/csm`, Dart в плагине), оба сверены с общим корпусом.
//! Гарантия совпадения для этого файла даётся тестом, который воспроизводит
//! эталонные документы корпуса байт в байт.

use std::collections::BTreeMap;

use super::cbor::{self, Value};

/// Версия спецификации в конверте.
pub const SPEC_VERSION: u64 = 1;

/// Сроки жизни по типам (`03-WIRE.md` 8.0). Панель не имеет права подписать
/// документ с большим сроком; меньший допустим.
pub const LIFETIME_KEY: u64 = 604_800;
pub const LIFETIME_CATALOG: u64 = 2_592_000;
pub const LIFETIME_DIRECTIVE: u64 = 3_600;
pub const LIFETIME_BOOTSTRAP: u64 = 2_592_000;
pub const LIFETIME_RESERVE: u64 = 604_800;

/// Предел кадра для типов без нарезки на части (`03-WIRE.md` 8.0.1).
pub const DOC_FRAME_MAX: usize = 4096;

/// Алгоритм ключа. В v1 единственный: Ed25519.
pub const ALG_ED25519: u64 = 1;

/// Роль подписи (`03-WIRE.md` раздел 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u64)]
pub enum Role {
    Root = 1,
    Online = 2,
}

/// Запись о ключе в ключевом документе.
#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub kid: [u8; 12],
    pub public_key: [u8; 32],
}

impl KeyEntry {
    fn to_value(&self) -> Value {
        Value::map([
            (1, Value::Bytes(self.kid.to_vec())),
            (2, Value::Uint(ALG_ED25519)),
            (3, Value::Bytes(self.public_key.to_vec())),
        ])
    }
}

/// Порог для роли: какие ключи допущены и сколько подписей нужно.
#[derive(Debug, Clone)]
pub struct RoleSpec {
    pub role: Role,
    pub kids: Vec<[u8; 12]>,
    pub threshold: u64,
}

/// Общий конверт: ключи 1..5, обязательные во всех документах.
fn envelope(pid: &[u8; 8], ver: u64, iat: u64, lifetime: u64) -> BTreeMap<u64, Value> {
    let mut m = BTreeMap::new();
    m.insert(1, Value::Uint(SPEC_VERSION));
    m.insert(2, Value::Bytes(pid.to_vec()));
    m.insert(3, Value::Uint(ver));
    m.insert(4, Value::Uint(iat));
    m.insert(5, Value::Uint(iat + lifetime));
    m
}

/// Ключевой документ, `doc_type = 0x01`. Подписывается корневым ключом и
/// является якорем доверия для всего остального.
#[derive(Debug, Clone)]
pub struct KeyDocument {
    pub pid: [u8; 8],
    pub ver: u64,
    pub iat: u64,
    pub keys: Vec<KeyEntry>,
    pub roles: Vec<RoleSpec>,
    /// Отозванные идентификаторы ключей.
    pub revoked_kids: Vec<[u8; 12]>,
    /// Отозванные идентификаторы узлов.
    pub revoked_nodes: Vec<String>,
    /// Хэш каталога по тиру: `tier -> sha256(frame)`. Обязателен к выпуску для
    /// каждого обслуживаемого тира, иначе клиент теряет привязку каталога к
    /// корню.
    pub tiers: BTreeMap<u64, [u8; 32]>,
    /// Секунды до следующего обновления документа.
    pub ttl: Option<u64>,
}

impl KeyDocument {
    /// Собирает значение полезной нагрузки.
    pub fn to_value(&self) -> Value {
        let mut m = envelope(&self.pid, self.ver, self.iat, LIFETIME_KEY);

        m.insert(
            10,
            Value::Array(self.keys.iter().map(KeyEntry::to_value).collect()),
        );

        let mut roles = BTreeMap::new();
        for r in &self.roles {
            roles.insert(
                r.role as u64,
                Value::map([
                    (
                        1,
                        Value::Array(r.kids.iter().map(|k| Value::Bytes(k.to_vec())).collect()),
                    ),
                    (2, Value::Uint(r.threshold)),
                ]),
            );
        }
        m.insert(11, Value::Map(roles));

        if !self.revoked_kids.is_empty() || !self.revoked_nodes.is_empty() {
            let mut rev = BTreeMap::new();
            if !self.revoked_kids.is_empty() {
                rev.insert(
                    1,
                    Value::Array(
                        self.revoked_kids
                            .iter()
                            .map(|k| Value::Bytes(k.to_vec()))
                            .collect(),
                    ),
                );
            }
            if !self.revoked_nodes.is_empty() {
                rev.insert(
                    2,
                    Value::Array(
                        self.revoked_nodes
                            .iter()
                            .map(|n| Value::Text(n.clone()))
                            .collect(),
                    ),
                );
            }
            m.insert(12, Value::Map(rev));
        }

        if !self.tiers.is_empty() {
            let tiers = self
                .tiers
                .iter()
                .map(|(tier, hash)| (*tier, Value::Bytes(hash.to_vec())))
                .collect::<BTreeMap<_, _>>();
            m.insert(13, Value::Map(tiers));
        }

        if let Some(ttl) = self.ttl {
            m.insert(16, Value::Uint(ttl));
        }

        Value::Map(m)
    }

    /// Кодирует полезную нагрузку, проверив пределы профиля.
    pub fn encode(&self) -> Result<Vec<u8>, cbor::LimitError> {
        let v = self.to_value();
        cbor::check(&v)?;
        Ok(cbor::encode(&v))
    }
}

/// Запись зеркала (`03-WIRE.md` 8.2, «Mirror entry»).
///
/// `asn` и `cc` панель определяет при сохранении пула и подписывает вместе с
/// остальным: клиент обязан иметь возможность ПРОВЕРИТЬ заявленное разнообразие
/// провайдеров, а не поверить ему на слово.
#[derive(Debug, Clone)]
pub struct Mirror {
    /// Хост.
    pub host: String,
    /// SNI, задаётся явно для каждого зеркала.
    pub sni: String,
    /// Пины SPKI (sha256), от одного до четырёх.
    pub pins: Vec<[u8; 32]>,
    /// Автономная система.
    pub asn: u64,
    /// Страна, две заглавные буквы ISO 3166-1.
    pub country: String,
    /// Вес выбора, 1..100. `None` означает 10.
    pub weight: Option<u64>,
    /// Литеральные адреса для ступени без резолвера.
    pub ips: Vec<String>,
}

impl Mirror {
    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(1, Value::Text(self.host.clone()));
        m.insert(2, Value::Text(self.sni.clone()));
        m.insert(
            3,
            Value::Array(self.pins.iter().map(|p| Value::Bytes(p.to_vec())).collect()),
        );
        m.insert(4, Value::Uint(self.asn));
        m.insert(5, Value::Text(self.country.clone()));
        if let Some(w) = self.weight {
            m.insert(6, Value::Uint(w));
        }
        if !self.ips.is_empty() {
            m.insert(
                7,
                Value::Array(self.ips.iter().map(|i| Value::Text(i.clone())).collect()),
            );
        }
        Value::Map(m)
    }
}

/// Запись DoH (`03-WIRE.md` 8.2, «DoH entry»).
///
/// Литеральные адреса обязательны именно здесь: ступень, которая поднимается
/// без резолвера, обязана уметь подключиться, не спрашивая DNS. Проверка
/// сертификата при этом не отключается: имя берётся из `host` и уходит в SNI.
#[derive(Debug, Clone)]
pub struct DohEntry {
    pub host: String,
    /// Путь запроса, только путь.
    pub path: String,
    pub ips: Vec<String>,
    pub pins: Vec<[u8; 32]>,
}

impl DohEntry {
    fn to_value(&self) -> Value {
        Value::map([
            (1, Value::Text(self.host.clone())),
            (2, Value::Text(self.path.clone())),
            (
                3,
                Value::Array(self.ips.iter().map(|i| Value::Text(i.clone())).collect()),
            ),
            (
                4,
                Value::Array(self.pins.iter().map(|p| Value::Bytes(p.to_vec())).collect()),
            ),
        ])
    }
}

/// Bootstrap-блоб, `doc_type = 0x05` (`03-WIRE.md` 8.5).
///
/// Подписывается корнем и живёт ВНЕ того хоста, от которого он спасает: это
/// набор для входа, когда основной адрес недоступен. Он самодостаточен, потому
/// что несёт сам корневой ключ, и пригоден к диктовке голосом в вырожденном
/// случае.
///
/// Цен, ссылок на оплату и имени бота здесь нет и быть не может.
#[derive(Debug, Clone)]
pub struct BootstrapBlob {
    pub pid: [u8; 8],
    pub ver: u64,
    pub iat: u64,
    /// Origin энроллмента, `https://host[:port]`, без пути.
    pub origin: String,
    /// Код приглашения с вплетённым пином.
    pub code: String,
    /// Корневой публичный ключ: блоб самодостаточен.
    pub root_public: [u8; 32],
    pub mirrors: Vec<Mirror>,
    pub doh: Vec<DohEntry>,
    /// Отображаемое имя оператора, инертный текст.
    pub operator_name: Option<String>,
}

impl BootstrapBlob {
    pub fn to_value(&self) -> Value {
        let mut m = envelope(&self.pid, self.ver, self.iat, LIFETIME_BOOTSTRAP);
        m.insert(10, Value::Text(self.origin.clone()));
        m.insert(11, Value::Text(self.code.clone()));
        m.insert(12, Value::Bytes(self.root_public.to_vec()));
        m.insert(
            13,
            Value::Array(self.mirrors.iter().map(Mirror::to_value).collect()),
        );
        m.insert(
            14,
            Value::Array(self.doh.iter().map(DohEntry::to_value).collect()),
        );
        if let Some(n) = &self.operator_name {
            m.insert(15, Value::Text(n.clone()));
        }
        Value::Map(m)
    }

    pub fn encode(&self) -> Result<Vec<u8>, cbor::LimitError> {
        let v = self.to_value();
        cbor::check(&v)?;
        Ok(cbor::encode(&v))
    }
}

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

/// Запись зеркала в bootstrap-блобе и в резервном пуле.
#[derive(Debug, Clone)]
pub struct Mirror {
    /// Хост зеркала.
    pub host: String,
    /// Порт. `None` означает 443.
    pub port: Option<u64>,
}

impl Mirror {
    fn to_value(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert(1, Value::Text(self.host.clone()));
        if let Some(p) = self.port {
            m.insert(2, Value::Uint(p));
        }
        Value::Map(m)
    }
}

/// Bootstrap-блоб, `doc_type = 0x05`. Подписывается корнем: это то, что клиент
/// получает при энроллменте, до того как у него есть хоть какое-то доверие.
#[derive(Debug, Clone)]
pub struct BootstrapBlob {
    pub pid: [u8; 8],
    pub ver: u64,
    pub iat: u64,
    /// Зеркала, с которых можно взять ключевой документ.
    pub mirrors: Vec<Mirror>,
    /// Идентификатор тира, к которому относится приглашение.
    pub tier: Option<u64>,
}

impl BootstrapBlob {
    pub fn to_value(&self) -> Value {
        let mut m = envelope(&self.pid, self.ver, self.iat, LIFETIME_BOOTSTRAP);
        if !self.mirrors.is_empty() {
            m.insert(
                10,
                Value::Array(self.mirrors.iter().map(Mirror::to_value).collect()),
            );
        }
        if let Some(t) = self.tier {
            m.insert(16, Value::Uint(t));
        }
        Value::Map(m)
    }

    pub fn encode(&self) -> Result<Vec<u8>, cbor::LimitError> {
        let v = self.to_value();
        cbor::check(&v)?;
        Ok(cbor::encode(&v))
    }
}

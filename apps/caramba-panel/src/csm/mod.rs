//! CSM/1 на стороне панели: личность оператора и выпуск подписанных документов.
//!
//! Спецификация: `apps/caramba-client/docs/protocol/02-SPEC.md` (нормативная),
//! `03-WIRE.md` (формат байт), `01-DECISION.md` раздел 7 (что именно должна
//! сделать панель и в каком порядке).
//!
//! Разделение ключей это суть модели доверия, а не бюрократия:
//!
//!   * КОРНЕВОЙ ключ живёт офлайн у оператора. Им подписываются только якорь
//!     доверия (ключевой документ), приглашение и резервный пул зеркал. Панель
//!     его не хранит и в норме не видит.
//!   * ОНЛАЙН-ключ живёт в секрете окружения работающей панели и подписывает
//!     то, что меняется каждый день: каталог и директивы.
//!
//! Смысл в том, что взлом работающей панели даёт злоумышленнику онлайн-ключ,
//! но не даёт подменить якорь: клиент пришпилен к корню, и восстановление
//! идёт через новый ключевой документ, подписанный офлайн-корнем.

use anyhow::{Context, Result, anyhow};
use caramba_shared::csm;
use ed25519_dalek::SigningKey;
use sqlx::PgPool;

// Загрузка ключей нужна маршрутам, подписываемым ОНЛАЙН-ключом (каталог и
// директива): они приземляются следующим шагом, поэтому часть API пока не
// вызывается из продакшн-кода, но покрыта тестами.
pub mod catalog_store;
#[allow(dead_code)]
pub mod keys;
pub mod routes;

/// Переменная окружения с секретом локатора (`03-WIRE.md` раздел 4,
/// `CSM_LOC_SECRET`): 32 байта в hex. Живёт рядом с онлайн-ключом и НЕ в
/// таблице настроек: его ротация меняет все локаторы тенанта разом, это
/// аварийный рычаг оператора, а не строка конфигурации.
pub const ENV_LOC_SECRET: &str = "CARAMBA_CSM_LOC_SECRET";

/// Секрет локатора из окружения. `None`, если переменная не задана: тогда
/// панель может обслуживать уже выданные локаторы, но не выдавать новые.
pub fn loc_secret_from_env() -> Result<Option<[u8; 32]>> {
    let raw = match std::env::var(ENV_LOC_SECRET) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(None),
    };
    from_hex::<32>(&raw)
        .map(Some)
        .with_context(|| format!("csm: {ENV_LOC_SECRET} должен быть 64 hex-символа (32 байта)"))
}

/// Личность тенанта для протокола.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TenantIdentity {
    /// `sha256(root_pk)[0..8]`, идентификатор тенанта в каждом документе.
    pub pid: [u8; 8],
    /// Публичный корневой ключ.
    pub root_public: [u8; 32],
    /// `keyid_trunc` корня.
    pub root_kid: [u8; 12],
}

/// Ключи, которыми панель может подписывать прямо сейчас.
#[allow(dead_code)]
pub struct SigningKeys {
    pub identity: TenantIdentity,
    /// Онлайн-ключ из секрета окружения. `None` означает, что панель не может
    /// выпускать каталоги и директивы: маршруты отвечают отказом, а не молча
    /// отдают неподписанное.
    pub online: Option<SigningKey>,
    /// `keyid_trunc` онлайн-ключа, если он есть.
    pub online_kid: Option<[u8; 12]>,
}

#[allow(dead_code)]
impl SigningKeys {
    /// Загружает личность из базы и онлайн-ключ из окружения.
    ///
    /// Отсутствие настроенного корня это не ошибка запуска: оператор может
    /// никогда не включать протокол. Маршруты CSM в этом случае отвечают 503 с
    /// внятной причиной, а панель работает как раньше.
    pub async fn load(pool: &PgPool) -> Result<Option<Self>> {
        let Some(identity) = keys::load_identity(pool).await? else {
            return Ok(None);
        };

        let online = match keys::online_key_from_env()? {
            Some(sk) => sk,
            None => {
                return Ok(Some(SigningKeys {
                    identity,
                    online: None,
                    online_kid: None,
                }));
            }
        };

        let kid = csm::keyid_trunc(&online.verifying_key().to_bytes());

        // Онлайн-ключ обязан быть заявлен в базе: иначе панель подпишет каталог
        // ключом, которого нет в ключевом документе, и клиент отвергнет кадр как
        // подписанный посторонним. Лучше узнать об этом при старте.
        let known = keys::key_exists(pool, &hex(&kid)).await?;
        if !known {
            return Err(anyhow!(
                "csm: онлайн-ключ {} не зарегистрирован в csm_keys; \
                 добавьте его публичную часть, иначе клиенты отвергнут подписи",
                hex(&kid)
            ));
        }

        Ok(Some(SigningKeys {
            identity,
            online: Some(online),
            online_kid: Some(kid),
        }))
    }

    /// Онлайн-ключ или внятная ошибка.
    pub fn require_online(&self) -> Result<&SigningKey> {
        self.online
            .as_ref()
            .context("csm: онлайн-ключ не настроен (CARAMBA_CSM_ONLINE_KEY)")
    }
}

/// Hex-представление байт: в базе и в логах идентификаторы ключей живут строкой.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Разбор hex-строки фиксированной длины.
pub fn from_hex<const N: usize>(s: &str) -> Result<[u8; N]> {
    let s = s.trim();
    if s.len() != N * 2 {
        return Err(anyhow!(
            "csm: ожидалось {} hex-символов, получено {}",
            N * 2,
            s.len()
        ));
    }
    let mut out = [0u8; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .with_context(|| format!("csm: невалидный hex в позиции {i}"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let bytes = [0x22u8, 0x6e, 0x8a, 0x20, 0xf6, 0x99, 0xb9, 0x64];
        let s = hex(&bytes);
        assert_eq!(s, "226e8a20f699b964");
        assert_eq!(from_hex::<8>(&s).unwrap(), bytes);
    }

    #[test]
    fn from_hex_rejects_wrong_length() {
        assert!(from_hex::<8>("226e8a20").is_err());
        assert!(from_hex::<8>("226e8a20f699b964zz").is_err());
    }
}

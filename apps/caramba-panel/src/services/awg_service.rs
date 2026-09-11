//! AmneziaWG, панельная половина.
//!
//! Почему отдельный сервис, а не ветка в orchestration_service: на ноде AWG это
//! НЕ инбаунд sing-box, а отдельный процесс amneziawg-go с интерфейсом awg0.
//! Стоковый sing-box не умеет wireguard-inbound с полями обфускации — такой
//! инбаунд валит `sing-box check` и кладёт весь конфиг узла. Поэтому сервер AWG
//! описывается своей таблицей `node_awg` и уезжает на ноду отдельным разделом
//! `awg` рядом с sing-box-конфигом, а не внутри него.
//!
//! Сервис — единственный источник правды по:
//!   * серверным ключам и параметрам обфускации ноды (`node_awg`),
//!   * клиентским ключам и адресам подписок (`subscription_awg_keys`),
//!   * зеркальной строке в `inbounds` (protocol = amneziawg), которая держит
//!     порт занятым в общем аллокаторе портов и питает генератор подписки.

use anyhow::Result;
use caramba_db::models::awg::NodeAwg;
use caramba_db::models::network::Inbound;
use caramba_db::repositories::awg_repo::AwgRepository;
use caramba_shared::config::{AwgPeer, AwgSection};
use sqlx::PgPool;
use tracing::{info, warn};

/// Тег зеркального инбаунда. Фиксированный: строка одна на ноду и находится
/// по тегу, а не по случайному суффиксу, как у шаблонных инбаундов.
pub const AWG_MIRROR_TAG: &str = "amneziawg-awg0";

/// Полоса UDP-портов для awg0. Следующая свободная за TUIC (16400-16499),
/// чтобы порт AWG нельзя было выдать sing-box-инбаунду и наоборот.
const AWG_PORT_START: i64 = 17400;
const AWG_PORT_END: i64 = 17499;

/// Адрес интерфейса awg0 вместе с маской пула пиров.
pub const AWG_SERVER_ADDRESS_CIDR: &str = "10.66.0.1/16";

/// Размер пула адресов: 252 третьих октета по 250 хостов.
const POOL_SPAN: i64 = 252 * 250;

/// Адрес пира по id подписки, из пула 10.66.0.0/16.
///
/// Детерминированно и без похода в БД: ровно эту же функцию зовёт генератор
/// подписки, поэтому адрес в клиентском конфиге и `allowed_ip` пира на ноде
/// физически не могут разойтись. Крайние .0/.1/.255 не выдаются.
pub fn awg_allowed_ip(subscription_id: i64) -> String {
    let n = subscription_id.rem_euclid(POOL_SPAN);
    let third = 1 + (n / 250);
    let fourth = 2 + (n % 250);
    format!("10.66.{}.{}", third, fourth)
}

/// Приватный ключ клиента: тот же вывод, что у
/// `subscription_service::generate_amneziawg_key`. Ключ выводится из uuid
/// подписки, поэтому обе половины (пир на ноде и конфиг в подписке) получают
/// одну пару, не сверяясь друг с другом.
pub fn derive_client_private_key(uuid: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(uuid.as_bytes());
    hasher.update(b"amneziawg-key-salt");
    let result = hasher.finalize();

    let mut key = [0u8; 32];
    key.copy_from_slice(&result[..32]);

    // Клампинг X25519: без него ключ не является валидным скаляром.
    key[0] &= 248;
    key[31] &= 127;
    key[31] |= 64;

    base64::Engine::encode(&base64::prelude::BASE64_STANDARD, key)
}

/// Публичный ключ из приватного (оба в стандартном base64, формат wg).
pub fn public_from_private(priv_b64: &str) -> String {
    use x25519_dalek::{PublicKey, StaticSecret};

    let bytes =
        base64::Engine::decode(&base64::prelude::BASE64_STANDARD, priv_b64).unwrap_or_default();
    if bytes.len() != 32 {
        return String::new();
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let secret = StaticSecret::from(arr);
    base64::Engine::encode(
        &base64::prelude::BASE64_STANDARD,
        PublicKey::from(&secret).as_bytes(),
    )
}

/// Свежая серверная пара ключей. Генерится в процессе панели, а не через
/// `sing-box generate`: панель не обязана иметь sing-box на диске.
fn generate_server_keypair() -> (String, String) {
    let mut bytes = rand::random::<[u8; 32]>();
    bytes[0] &= 248;
    bytes[31] &= 127;
    bytes[31] |= 64;
    let priv_b64 = base64::Engine::encode(&base64::prelude::BASE64_STANDARD, bytes);
    let pub_b64 = public_from_private(&priv_b64);
    (priv_b64, pub_b64)
}

/// Параметры обфускации AmneziaWG. Разные на каждую ноду: одинаковые сигнатуры
/// на всех узлах это ровно то, что DPI и блокирует.
///
/// H1..H4 подчиняются требованию агента (apps/caramba-node/src/awg/mod.rs::validate):
/// каждый строго больше 4 и все четыре попарно различны. Значения 1..4 заняты
/// штатными типами пакетов WireGuard, а совпадение двух H между собой делает
/// разбор неоднозначным. Узел проверяет это fail-closed и отвергает ВЕСЬ раздел
/// awg, поэтому «почти невероятная» коллизия из rng здесь недопустима: цена
/// ошибки — молча не поднявшийся AWG на узле до ручной перегенерации строки.
fn generate_params() -> (i32, i32, i32, i32, i32, i64, i64, i64, i64) {
    use rand::Rng;
    let mut rng = rand::rng();
    let [h1, h2, h3, h4] = generate_header_types(&mut rng);
    (
        rng.random_range(3..=10),
        rng.random_range(40..=100),
        rng.random_range(500..=1000),
        rng.random_range(20..=100),
        rng.random_range(20..=100),
        h1,
        h2,
        h3,
        h4,
    )
}

/// Четыре попарно различных типа заголовка, каждый строго больше 4.
///
/// Диапазон начинается с 5 именно потому, что 1..4 — штатные типы WireGuard.
fn generate_header_types<R: rand::Rng + ?Sized>(rng: &mut R) -> [i64; 4] {
    let mut out: Vec<i64> = Vec::with_capacity(4);
    while out.len() < 4 {
        let candidate = rng.random_range(5u32..=u32::MAX) as i64;
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    [out[0], out[1], out[2], out[3]]
}

#[derive(Clone)]
pub struct AwgService {
    pool: PgPool,
    repo: AwgRepository,
}

impl AwgService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: AwgRepository::new(pool.clone()),
            pool,
        }
    }

    /// Подтягивает глобальный тумблер `amneziawg_enabled` из настроек в
    /// синхронное зеркало `utils`. Генераторы подписки синхронные и в БД
    /// сходить не могут, поэтому зеркало обновляется здесь — на каждом
    /// heartbeat и на каждой выдаче конфига узла, то есть не реже раза в
    /// интервал агента.
    pub async fn refresh_gate(&self) -> bool {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'amneziawg_enabled'")
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None);
        let enabled = matches!(value.as_deref(), Some("true") | Some("1") | Some("on"));
        crate::utils::set_amneziawg_enabled(enabled);
        enabled
    }

    /// Возвращает строку `node_awg`, создавая её при первом обращении.
    /// Ключи и параметры генерируются РОВНО один раз: их смена рвёт хендшейк
    /// всем уже выданным клиентам разом.
    pub async fn ensure_node_awg(&self, node_id: i64) -> Result<NodeAwg> {
        if let Some(existing) = self.repo.get_node_awg(node_id).await? {
            return Ok(existing);
        }

        let port = self.allocate_port(node_id).await?;
        let (priv_key, pub_key) = generate_server_keypair();
        let (jc, jmin, jmax, s1, s2, h1, h2, h3, h4) = generate_params();

        let now = chrono::Utc::now();
        let draft = NodeAwg {
            node_id,
            listen_port: port as i32,
            private_key: priv_key,
            public_key: pub_key,
            address_cidr: AWG_SERVER_ADDRESS_CIDR.to_string(),
            jc,
            jmin,
            jmax,
            s1,
            s2,
            h1,
            h2,
            h3,
            h4,
            // Выключено по умолчанию: включает оператор тумблером в карточке.
            enabled: false,
            created_at: now,
            updated_at: now,
        };

        let stored = self.repo.insert_node_awg_if_absent(&draft).await?;
        info!(
            "AmneziaWG: узлу {} выдан сервер awg0 на порту {}",
            node_id, stored.listen_port
        );
        Ok(stored)
    }

    pub async fn set_enabled(&self, node_id: i64, enabled: bool) -> Result<()> {
        self.ensure_node_awg(node_id).await?;
        self.repo.set_node_awg_enabled(node_id, enabled).await
    }

    pub async fn get(&self, node_id: i64) -> Result<Option<NodeAwg>> {
        self.repo.get_node_awg(node_id).await
    }

    /// Свободный порт в полосе AWG. Занятость считается по тем же `inbounds`,
    /// что и у sing-box, поэтому два слушателя не сядут на один порт.
    async fn allocate_port(&self, node_id: i64) -> Result<i64> {
        let used: Vec<i64> = sqlx::query_scalar(
            "SELECT listen_port FROM inbounds WHERE node_id = $1
             UNION SELECT listen_port::bigint FROM node_awg WHERE node_id = $1",
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for port in AWG_PORT_START..=AWG_PORT_END {
            if !used.contains(&port) {
                return Ok(port);
            }
        }
        Err(anyhow::anyhow!(
            "нет свободного порта для AmneziaWG на узле {} в полосе {}-{}",
            node_id,
            AWG_PORT_START,
            AWG_PORT_END
        ))
    }

    /// Приводит зеркальную строку `inbounds` к состоянию `node_awg`.
    ///
    /// Зачем зеркало вообще: генераторы подписки ходят по `node.inbounds` и в
    /// БД сходить не могут. Строка держит порт занятым, несёт публичный ключ
    /// сервера и параметры обфускации, и её `enable` это итог двух тумблеров
    /// (глобального в настройках и пер-нодового). Приватного ключа сервера в
    /// ней нет намеренно: раздел `awg` конфига узла единственное место, куда
    /// он уезжает.
    pub async fn sync_mirror_inbound(&self, node_id: i64, awg: &NodeAwg, globally_enabled: bool) {
        let enable = globally_enabled && awg.enabled;

        let settings = serde_json::json!({
            "protocol": "amneziawg",
            "users": [],
            "private_key": "",
            "public_key": awg.public_key,
            "listen_port": awg.listen_port,
            "jc": awg.jc,
            "jmin": awg.jmin,
            "jmax": awg.jmax,
            "s1": awg.s1,
            "s2": awg.s2,
            "h1": awg.h1,
            "h2": awg.h2,
            "h3": awg.h3,
            "h4": awg.h4,
        })
        .to_string();

        // Порт мог смениться (ручная правка, перевыдача) — старое зеркало на
        // другом порту иначе осталось бы висеть и раздавать мёртвый endpoint.
        let _ = sqlx::query(
            "DELETE FROM inbounds WHERE node_id = $1 AND protocol = 'amneziawg' AND listen_port <> $2",
        )
        .bind(node_id)
        .bind(awg.listen_port as i64)
        .execute(&self.pool)
        .await;

        let res = sqlx::query(
            r#"
            INSERT INTO inbounds (node_id, tag, protocol, listen_port, listen_ip, settings, stream_settings, remark, enable)
            VALUES ($1, $2, 'amneziawg', $3, '0.0.0.0', $4, '{}', 'AmneziaWG (awg0)', $5)
            ON CONFLICT (node_id, listen_port) DO UPDATE SET
                tag = EXCLUDED.tag,
                protocol = EXCLUDED.protocol,
                settings = EXCLUDED.settings,
                enable = EXCLUDED.enable,
                remark = EXCLUDED.remark
            "#,
        )
        .bind(node_id)
        .bind(AWG_MIRROR_TAG)
        .bind(awg.listen_port as i64)
        .bind(&settings)
        .bind(enable)
        .execute(&self.pool)
        .await;

        if let Err(e) = res {
            warn!(
                "AmneziaWG: не удалось синхронизировать зеркальный inbound узла {}: {}",
                node_id, e
            );
        }
    }

    /// Собирает раздел `awg` конфига узла. `None` = строки `node_awg` нет,
    /// узлу про AWG знать нечего.
    pub async fn build_section(&self, node_id: i64) -> Result<Option<AwgSection>> {
        let globally_enabled = crate::utils::amneziawg_enabled();
        let Some(awg) = self.repo.get_node_awg(node_id).await? else {
            return Ok(None);
        };

        let enabled = globally_enabled && awg.enabled;
        let peers = if enabled {
            self.build_peers(node_id).await?
        } else {
            // Выключено — раздел всё равно уезжает, чтобы агент погасил awg0,
            // а не оставил поднятым интерфейс с прежними пирами.
            Vec::new()
        };

        Ok(Some(AwgSection {
            enabled,
            listen_port: awg.listen_port.clamp(0, u16::MAX as i32) as u16,
            private_key: awg.private_key,
            address_cidr: awg.address_cidr,
            jc: awg.jc.clamp(0, u16::MAX as i32) as u16,
            jmin: awg.jmin.clamp(0, u16::MAX as i32) as u16,
            jmax: awg.jmax.clamp(0, u16::MAX as i32) as u16,
            s1: awg.s1.clamp(0, u16::MAX as i32) as u16,
            s2: awg.s2.clamp(0, u16::MAX as i32) as u16,
            h1: awg.h1.clamp(0, u32::MAX as i64) as u32,
            h2: awg.h2.clamp(0, u32::MAX as i64) as u32,
            h3: awg.h3.clamp(0, u32::MAX as i64) as u32,
            h4: awg.h4.clamp(0, u32::MAX as i64) as u32,
            peers,
        }))
    }

    /// Пиры узла: активные подписки тех планов, которым этот узел вообще выдан.
    /// Набор тот же, что у sing-box-инбаундов узла, иначе AWG раздавал бы
    /// доступ шире, чем остальные протоколы.
    async fn build_peers(&self, node_id: i64) -> Result<Vec<AwgPeer>> {
        let rows = sqlx::query_as::<_, (i64, Option<String>, Option<i64>, i64)>(
            r#"
            SELECT DISTINCT s.id, s.vless_uuid, u.tg_id, u.id
            FROM subscriptions s
            JOIN users u ON u.id = s.user_id
            JOIN plan_inbounds pi ON pi.plan_id = s.plan_id
            JOIN inbounds i ON i.id = pi.inbound_id
            WHERE i.node_id = $1 AND LOWER(s.status) = 'active'
            "#,
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;

        let mut peers = Vec::with_capacity(rows.len());
        for (sub_id, uuid, tg_id, user_id) in rows {
            let Some(uuid) = uuid.filter(|u| !u.trim().is_empty()) else {
                continue;
            };
            let identity = crate::services::user_tag::config_client_identity(tg_id, user_id);
            let private_key = derive_client_private_key(&uuid);
            let public_key = public_from_private(&private_key);
            if public_key.is_empty() {
                continue;
            }
            let allowed_ip = format!("{}/32", awg_allowed_ip(sub_id));

            // Журнал выданного: значения детерминированы, поэтому запись здесь
            // не создаёт нового секрета, но даёт откуда посмотреть, какой ключ
            // и адрес у подписки, не вычисляя их заново.
            if let Err(e) = self
                .repo
                .upsert_subscription_key(sub_id, &public_key, &private_key, &allowed_ip)
                .await
            {
                warn!(
                    "AmneziaWG: не удалось записать ключи подписки {}: {}",
                    sub_id, e
                );
            }

            peers.push(AwgPeer {
                public_key,
                allowed_ip,
                user_tag: crate::services::user_tag::user_tag(identity),
            });
        }

        // Порядок стабильный: иначе хеш раздела прыгал бы на каждой выдаче
        // конфига и агент бесконечно «применял» неизменившийся список.
        peers.sort_by(|a, b| a.public_key.cmp(&b.public_key));
        Ok(peers)
    }

    /// Записывает heartbeat.active_users. Пустой список сюда не попадает:
    /// «нода ничего не прислала» не равно «все офлайн».
    pub async fn record_activity(
        &self,
        node_id: i64,
        users: &[caramba_shared::api::ActiveUser],
    ) -> Result<()> {
        if users.is_empty() {
            return Ok(());
        }
        let rows: Vec<(String, i64, i64, bool)> = users
            .iter()
            .filter(|u| !u.tag.trim().is_empty())
            .map(|u| {
                (
                    u.tag.clone(),
                    u.rx_delta.min(i64::MAX as u64) as i64,
                    u.tx_delta.min(i64::MAX as u64) as i64,
                    u.online,
                )
            })
            .collect();
        self.repo.upsert_user_activity(node_id, &rows).await
    }
}

/// Хеш раздела `awg`. Отдельный от хеша sing-box-конфига: смена состава пиров
/// обязана доезжать до ноды, но НЕ обязана перезапускать sing-box.
pub fn section_hash(section: &AwgSection) -> String {
    let body = serde_json::to_string(section).unwrap_or_default();
    format!("{:x}", md5::compute(body.as_bytes()))
}

/// Зеркальный инбаунд AWG среди прочих. Нужен генератору конфига узла, который
/// его выбрасывает из sing-box-конфига.
pub fn is_awg_inbound(inbound: &Inbound) -> bool {
    inbound.protocol.eq_ignore_ascii_case("amneziawg")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Общая с узлом фикстура контракта.
    ///
    /// Тот же файл читает `awg::tests::contract_fixture_*` в агенте. Пока обе
    /// стороны тянут один JSON, разъехаться именами полей, типами или
    /// единицами они могут только вместе с падающим тестом.
    fn contract_fixture() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../libs/caramba-shared/testdata/awg_node_section.json"
        );
        let raw = std::fs::read_to_string(path).expect("фикстура контракта AWG");
        serde_json::from_str(&raw).expect("фикстура — валидный JSON")
    }

    /// Панель обязана уметь произвести ровно ту фикстуру, которую разбирает узел.
    #[test]
    fn awg_contract_fixture() {
        use caramba_shared::config::ConfigResponse;

        let fixture = contract_fixture();
        let resp: ConfigResponse =
            serde_json::from_value(fixture["config_response"].clone()).expect("ConfigResponse");
        let section = resp.awg.expect("раздел awg");

        // 1. Хеш раздела считает панель. Если он разойдётся с записанным в
        //    фикстуре, узел будет считать неизменившийся раздел изменившимся
        //    (или наоборот) — и мы этого не заметим до прода.
        assert_eq!(
            section_hash(&section),
            resp.awg_hash.expect("awg_hash"),
            "хеш раздела разошёлся с фикстурой"
        );

        // 2. Серверный ключ: приватный едет узлу, публичный — клиентам. Пара
        //    обязана сходиться, иначе хендшейка не будет ни у кого.
        let seed = fixture["derivation"]["server_private_key_seed"]
            .as_str()
            .unwrap();
        assert_eq!(section.private_key, derive_client_private_key(seed));
        assert_eq!(
            public_from_private(&section.private_key),
            fixture["derivation"]["server_public_key"].as_str().unwrap()
        );

        // 3. Каждый пир выводится панелью из подписки. Пересчитываем теми же
        //    функциями, которыми их строит build_peers, и сверяем с байтами,
        //    которые реально увидит узел.
        let derived = fixture["derivation"]["peers"].as_array().unwrap();
        assert_eq!(derived.len(), section.peers.len());
        for d in derived {
            let sub_id = d["subscription_id"].as_i64().unwrap();
            let uuid = d["subscription_uuid"].as_str().unwrap();
            let identity = d["client_identity"].as_i64().unwrap();

            let public_key = public_from_private(&derive_client_private_key(uuid));
            let peer = section
                .peers
                .iter()
                .find(|p| p.public_key == public_key)
                .unwrap_or_else(|| panic!("пир подписки {sub_id} не найден в разделе"));

            assert_eq!(peer.allowed_ip, format!("{}/32", awg_allowed_ip(sub_id)));
            assert_eq!(peer.user_tag, crate::services::user_tag::user_tag(identity));
            assert_eq!(
                crate::services::user_tag::parse_user_tag(&peer.user_tag),
                Some(identity),
                "тег обязан разбираться обратно, в том числе отрицательный"
            );
        }

        // 4. Порядок пиров — по публичному ключу. Иначе хеш раздела прыгал бы
        //    на каждой выдаче конфига и узел применял бы неизменившийся список.
        let keys: Vec<&str> = section
            .peers
            .iter()
            .map(|p| p.public_key.as_str())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);

        // 5. Параметры обфускации переживают дорогу в БД (BIGINT) и обратно:
        //    H выше i32::MAX — штатное значение, а не край.
        assert!(section.h4 > i32::MAX as u32);
        assert!(section.jmin <= section.jmax);
    }

    /// Heartbeat из той же фикстуры разбирается панелью, а дельты ложатся в
    /// `node_user_activity` без переполнения.
    #[test]
    fn awg_contract_fixture_heartbeat() {
        use caramba_shared::api::HeartbeatRequest;

        let fixture = contract_fixture();
        let hb: HeartbeatRequest =
            serde_json::from_value(fixture["heartbeat_request"].clone()).expect("HeartbeatRequest");
        let active = hb.active_users.expect("active_users");
        assert_eq!(active.len(), 3);

        // Ровно то преобразование, которое делает record_activity перед
        // upsert: u64 с узла в i64 столбца.
        let rows: Vec<(String, i64, i64, bool)> = active
            .iter()
            .filter(|u| !u.tag.trim().is_empty())
            .map(|u| {
                (
                    u.tag.clone(),
                    u.rx_delta.min(i64::MAX as u64) as i64,
                    u.tx_delta.min(i64::MAX as u64) as i64,
                    u.online,
                )
            })
            .collect();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|(_, rx, tx, _)| *rx >= 0 && *tx >= 0));

        // Трафик из active_users панель НЕ начисляет: байты уже пришли в
        // user_usage. Проверяем, что фикстура это и фиксирует — сумма
        // направлений равна записи в user_usage.
        let usage = hb.user_usage.expect("user_usage");
        for u in &active {
            let total = u.rx_delta + u.tx_delta;
            match usage.get(&u.tag) {
                Some(v) => assert_eq!(*v, total, "user_usage для {} разошёлся", u.tag),
                // Молчащий, но онлайновый пир в user_usage не попадает вовсе.
                None => assert_eq!(total, 0, "{} прокачал байты мимо user_usage", u.tag),
            }
        }
    }

    /// H1..H4 обязаны проходить валидацию агента: каждый больше 4 и все
    /// попарно различны. Иначе узел отвергает ВЕСЬ раздел awg fail-closed.
    #[test]
    fn generated_header_types_satisfy_the_node() {
        let mut rng = rand::rng();
        for _ in 0..2_000 {
            let h = generate_header_types(&mut rng);
            for (i, v) in h.iter().enumerate() {
                assert!(*v > 4, "h{} = {}", i + 1, v);
                assert!(*v <= u32::MAX as i64, "h{} вне u32: {}", i + 1, v);
            }
            for i in 0..h.len() {
                for j in (i + 1)..h.len() {
                    assert_ne!(h[i], h[j], "h{} и h{} совпали", i + 1, j + 1);
                }
            }
        }
    }

    #[test]
    fn pool_addresses_stay_inside_bounds() {
        for id in [0i64, 1, 249, 250, 251, 62_999, 63_000, -7] {
            let ip = awg_allowed_ip(id);
            let parts: Vec<u32> = ip.split('.').map(|p| p.parse().unwrap()).collect();
            assert_eq!(parts.len(), 4, "{ip}");
            assert_eq!(parts[0], 10);
            assert_eq!(parts[1], 66);
            assert!((1..=252).contains(&parts[2]), "третий октет {ip}");
            assert!((2..=251).contains(&parts[3]), "четвёртый октет {ip}");
        }
    }

    #[test]
    fn pool_address_is_deterministic_and_distinct() {
        assert_eq!(awg_allowed_ip(42), awg_allowed_ip(42));
        assert_ne!(awg_allowed_ip(42), awg_allowed_ip(43));
        assert_eq!(awg_allowed_ip(1), "10.66.1.3");
    }

    #[test]
    fn client_key_matches_subscription_generator() {
        // Обе половины обязаны выводить одну пару из uuid подписки.
        let uuid = "3f8a1c22-0000-4000-8000-000000000001";
        let a = derive_client_private_key(uuid);
        let b = derive_client_private_key(uuid);
        assert_eq!(a, b);

        let pubkey = public_from_private(&a);
        assert!(!pubkey.is_empty());
        assert_eq!(pubkey, public_from_private(&a));

        let raw = base64::Engine::decode(&base64::prelude::BASE64_STANDARD, &a).unwrap();
        assert_eq!(raw.len(), 32);
        // Клампинг X25519.
        assert_eq!(raw[0] & 7, 0);
        assert_eq!(raw[31] & 128, 0);
        assert_eq!(raw[31] & 64, 64);
    }

    #[test]
    fn server_keypair_is_consistent() {
        let (priv_b64, pub_b64) = generate_server_keypair();
        assert_eq!(public_from_private(&priv_b64), pub_b64);
        assert_ne!(priv_b64, generate_server_keypair().0);
    }

    #[test]
    fn public_key_from_garbage_is_empty() {
        assert_eq!(public_from_private(""), "");
        assert_eq!(public_from_private("not-base64!!"), "");
        assert_eq!(public_from_private("YWJj"), "");
    }

    #[test]
    fn section_hash_tracks_content_only() {
        let mut section = AwgSection {
            enabled: true,
            listen_port: 17400,
            private_key: "cA==".to_string(),
            address_cidr: AWG_SERVER_ADDRESS_CIDR.to_string(),
            jc: 5,
            jmin: 50,
            jmax: 700,
            s1: 30,
            s2: 40,
            h1: 1,
            h2: 2,
            h3: 3,
            h4: 4,
            peers: vec![],
        };
        let base = section_hash(&section);
        assert_eq!(base, section_hash(&section));

        section.peers.push(AwgPeer {
            public_key: "k".to_string(),
            allowed_ip: "10.66.1.2/32".to_string(),
            user_tag: "user_1".to_string(),
        });
        assert_ne!(base, section_hash(&section), "новый пир обязан менять хеш");
    }
}

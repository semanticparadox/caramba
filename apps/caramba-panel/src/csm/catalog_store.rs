//! Хранилище подписанных каталогов, локаторы подписок и сборка директивы.
//!
//! Три обязательства панели, которые этот модуль закрывает:
//!
//!   * `03-WIRE.md` 1.5: каталог тира подписывается ТОЛЬКО когда изменился
//!     дайджест его содержимого. Подписанный кадр и его части хранятся под
//!     `(tier, content_digest)`, `iat` это момент изменения содержимого, а
//!     отдача части это чтение строки, а не подпись. Иначе опубликованный в
//!     ключевом документе хэш тира протухал бы при каждом перезапуске.
//!     Ключ хранения связан с подписантом и тенантом
//!     ([`Catalog::storage_digest`]): ротация онлайн-ключа обязана дать одну
//!     переподпись, иначе панель отдавала бы кадр под отозванным `kid`.
//!     Второе исключение из «только по дайджесту» это срок жизни: каталог
//!     живёт 30 дней, и спокойный флот переподписывается заранее, до того
//!     как истечёт кадр, который называет каждая директива.
//!   * `03-WIRE.md` раздел 4: локатор это HMAC, его нельзя обратить, поэтому
//!     подписка ищется по индексной колонке `csm_subscriptions.locator`.
//!   * `02-SPEC.md` 4.7: версия директивы это счётчик на локатор, выделяемый
//!     одним `UPDATE ... RETURNING`, а не часы.
//!
//! Модель тира собирается из тех же узлов и инбаундов, из которых генератор
//! Clash собирает прокси, и через тот же разбор `stream_settings`: имя прокси
//! (`pn`) обязано совпадать с выпуском генератора дословно, иначе клиент не
//! найдёт свой выбор в легаси-конфиге.

use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use caramba_shared::csm::catalog::{
    Alpn, Catalog, CatalogError, Congestion, Fingerprint, Flow, Network, Node, Protocol, Security,
    SignedCatalog, SsMethod, Thresholds, cap,
};
use caramba_shared::csm::directive::{
    Capabilities, DeviceThumbprint, Directive, Locator, Nonce, ReasonCode, RelayResolution,
    Selection, Status, Traffic, crockford_decode,
};
use caramba_shared::csm::{self, LIFETIME_CATALOG};
use ed25519_dalek::SigningKey;
use sqlx::PgPool;

use super::{from_hex, hex};
use crate::AppState;
use crate::singbox::subscription_generator::{
    NodeInfo, format_node_label, format_proto_label, parse_stream_settings,
};

/// Период обновления директивы и джиттер по умолчанию (`03-WIRE.md` 11.6).
pub const TIER_TTL: u64 = 7200;
pub const TIER_JITTER: u64 = 20;
/// Диапазон корзин набивки тенанта по умолчанию (`03-WIRE.md` 12.2).
pub const PAD_BUCKETS: [u64; 2] = [0, 3];

/// Предел числа тиров тенанта: `tiers` ключевого документа вмещает 16 пар, и
/// тир без опубликованного хэша не имеет V14b (`02-SPEC.md` 4.4).
pub const MAX_TIERS: i64 = 16;

/// Запас до истечения каталога, при котором тир переподписывается заранее:
/// сутки кэша части (`Cache-Control` 13.4) плюс два периода директивы плюс
/// допуск часов. Директива, выданная за этот срок до истечения, называет
/// каталог, который клиент ещё успеет забрать и проверить.
pub fn renew_margin(directive_ttl: u64) -> u64 {
    86_400 + 2 * directive_ttl + 300
}

/// Страна для узла без корректного кода: ISO 3166-1 резервирует `ZZ` как
/// «неизвестно», и клиент рисует глобус, как генератор Clash.
const UNKNOWN_COUNTRY: &str = "ZZ";

/// MTU wireguard, который генератор Clash выпускает для amneziawg.
const WIREGUARD_MTU: u64 = 1280;

// ---------------------------------------------------------------- модель тира

/// Тир каталога это идентификатор плана: у панели нет другой сущности, которая
/// решала бы, какие узлы видит подписка. Диапазон 1..1023 задан `03-WIRE.md`
/// 8.1, и план за его пределами протокол обслужить не может.
pub fn tier_of_plan(plan_id: i64) -> Result<u64> {
    let tier = u64::try_from(plan_id).unwrap_or(0);
    if !(1..=1023).contains(&tier) {
        bail!("план {plan_id} вне диапазона тиров 1..1023");
    }
    Ok(tier)
}

/// Биты возможностей, которые панель реализует сегодня. Бит ставится только
/// над работающей функцией (`01-DECISION.md` B1): запечатывания, записи
/// настроек, пула зеркал, DoH, хэшей ресурсов и цепочек релэев пока нет, и
/// клиент обязан узнать об этом из подписанного поля, а не из ошибки.
/// Бит 8 стоит над бесплатным планом: это и есть onboarding-грант панели,
/// и его подписчики получают `st = onboarding`.
pub fn implemented_capabilities(exits: usize, free_plan: bool) -> u32 {
    let mut bits = 0;
    if exits > 0 {
        bits |= cap::NODE_MATERIAL;
    }
    if free_plan {
        bits |= cap::ONBOARDING_GRANT;
    }
    bits
}

/// Собирает модель тира из узлов плана. `ver` и `iat` здесь нули: их
/// выставляет решение о подписи, когда известно, менялось ли содержимое.
pub async fn load_tier_model(state: &AppState, pid: [u8; 8], tier: u64) -> Result<Catalog> {
    let nodes = state
        .store_service
        .node_repo
        .get_nodes_for_plan(tier as i64)
        .await
        .context("csm: узлы плана")?;
    let infos = state
        .subscription_service
        .get_node_infos_with_relays(&nodes)
        .await
        .context("csm: инбаунды узлов")?;
    let free_plan: bool =
        sqlx::query_scalar("SELECT COALESCE(is_free, FALSE) FROM plans WHERE id = $1")
            .bind(tier as i64)
            .fetch_optional(&state.pool)
            .await
            .context("csm: план тира")?
            .unwrap_or(false);

    let mut exits = Vec::new();
    for info in infos.iter().filter(|n| !n.is_relay) {
        exits.extend(node_entries(info, pid, tier));
    }
    Ok(tier_catalog(pid, tier, exits, free_plan))
}

/// Число действующих планов: каждый это тир, и их больше `MAX_TIERS` быть не
/// может, иначе часть тиров останется без хэша в ключевом документе.
pub async fn count_tiers(pool: &PgPool) -> Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM plans WHERE COALESCE(is_active, TRUE)")
        .fetch_one(pool)
        .await
        .context("csm: число планов")
}

/// Каталог тира с полями, не зависящими от флота.
pub fn tier_catalog(pid: [u8; 8], tier: u64, exits: Vec<Node>, free_plan: bool) -> Catalog {
    let cap = implemented_capabilities(exits.len(), free_plan);
    Catalog {
        pid,
        ver: 0,
        iat: 0,
        tier,
        exits,
        relays: Vec::new(),
        routes: Vec::new(),
        cap,
        mirrors: Vec::new(),
        doh: Vec::new(),
        rulesets: Vec::new(),
        geo: Vec::new(),
        ttl: TIER_TTL,
        jitter: TIER_JITTER,
        thresholds: Thresholds::default(),
        pad_buckets: PAD_BUCKETS,
        ladder: None,
        pins: Vec::new(),
        hpke: None,
    }
}

/// Записи узла для каждого инбаунда, который генератор Clash выпустил бы как
/// прокси. Запись, которую клиент отверг бы при разборе, не выпускается, а
/// логируется: один неверно настроенный инбаунд не должен оставить тир без
/// каталога.
pub fn node_entries(info: &NodeInfo, pid: [u8; 8], tier: u64) -> Vec<Node> {
    let mut out = Vec::new();
    for inbound in info.inbounds.iter().filter(|i| i.enable) {
        match node_entry(info, inbound) {
            Ok(Some(node)) => match entry_error(pid, tier, &node) {
                None => out.push(node),
                Some(e) => tracing::warn!(
                    node = %info.name,
                    inbound = inbound.id,
                    error = %e,
                    "csm: запись узла не проходит проверку и не выпускается"
                ),
            },
            Ok(None) => {}
            Err(reason) => tracing::warn!(
                node = %info.name,
                inbound = inbound.id,
                reason,
                "csm: инбаунд пропущен"
            ),
        }
    }
    out
}

/// Проверяет одну запись правилами 8.2.1 через одноэлементный каталог:
/// правила живут в общем крейте и не экспортированы по одной.
fn entry_error(pid: [u8; 8], tier: u64, node: &Node) -> Option<CatalogError> {
    tier_catalog(pid, tier, vec![node.clone()], false)
        .validate()
        .err()
}

/// `Ok(None)` означает «генератор Clash этот инбаунд тоже не выпускает», и
/// такой пропуск не логируется: у него нет имени прокси, которое клиент мог
/// бы искать.
fn node_entry(
    info: &NodeInfo,
    inbound: &caramba_db::models::network::Inbound,
) -> Result<Option<Node>, &'static str> {
    let si = parse_stream_settings(&inbound.stream_settings, info);
    if matches!(si.network.as_str(), "xhttp" | "splithttp") {
        return Ok(None);
    }
    let proto = inbound.protocol.to_ascii_lowercase();
    let protocol = match proto.as_str() {
        "vless" => Protocol::Vless,
        "vmess" => Protocol::Vmess,
        "trojan" => Protocol::Trojan,
        "hysteria2" | "hy2" => Protocol::Hysteria2,
        "tuic" => Protocol::Tuic,
        "shadowsocks" | "ss" => Protocol::Shadowsocks,
        "amneziawg" if crate::utils::amneziawg_client_enabled() => Protocol::Wireguard,
        _ => return Ok(None),
    };
    // Имя дословно как у генератора Clash, включая суффикс цепочки.
    let relay_suffix = if info.relay_info.is_some() {
        " ↪"
    } else {
        ""
    };
    let pn = format!(
        "{} {}{}",
        format_node_label(info),
        format_proto_label(&inbound.protocol, &si),
        relay_suffix
    );

    let network = match si.network.as_str() {
        // hysteria2 и tuic это всегда QUIC; `network` в их stream_settings
        // часто не задан, и разбор подставляет `tcp`, который здесь был бы
        // ложью про транспорт.
        _ if matches!(protocol, Protocol::Hysteria2 | Protocol::Tuic) => Network::Quic,
        "tcp" => Network::Tcp,
        "ws" => Network::Ws,
        "grpc" => Network::Grpc,
        "httpupgrade" => Network::HttpUpgrade,
        "quic" | "udp" => Network::Quic,
        _ => return Err("транспорт вне словаря nw"),
    };
    let security = match si.security.as_str() {
        "reality" => Security::Reality,
        "tls" => Security::Tls,
        _ if matches!(protocol, Protocol::Hysteria2 | Protocol::Tuic) => Security::Tls,
        _ => Security::None,
    };
    let port = u16::try_from(inbound.listen_port).map_err(|_| "порт вне 1..65535")?;
    let host = match protocol {
        Protocol::Wireguard => info.address.clone(),
        _ => info
            .frontend_url
            .clone()
            .unwrap_or_else(|| info.address.clone()),
    };

    let mut node = Node::new(
        format!("n{}i{}", inbound.node_id, inbound.id),
        pn,
        country_of(info.country_code.as_deref()),
        host.to_ascii_lowercase(),
        port,
        protocol,
        network,
        security,
    );

    match protocol {
        Protocol::Vless | Protocol::Vmess | Protocol::Trojan => {
            if security != Security::None {
                node.sni = Some(si.sni.to_ascii_lowercase());
                node.insecure = Some(false);
            }
            if security == Security::Reality {
                node.public_key = Some(reality_key(&si.public_key)?);
                if !si.short_id.is_empty() {
                    node.short_id = Some(si.short_id.clone());
                }
            }
            // Отпечаток там, где его выпускает генератор Clash: у VLESS при
            // любом TLS, у Trojan только под Reality, у VMess никогда.
            let clash_emits_fp = match protocol {
                Protocol::Vless => security != Security::None,
                Protocol::Trojan => security == Security::Reality,
                _ => false,
            };
            if clash_emits_fp {
                node.fingerprint = fingerprint_of(&si.fingerprint);
            }
            if protocol == Protocol::Vless && si.flow == "xtls-rprx-vision" {
                node.flow = Some(Flow::Vision);
            }
            match network {
                Network::Ws | Network::HttpUpgrade => {
                    node.path = Some(si.ws_path.clone());
                    // Генератор Clash шлёт `Host: sni` в ws-opts и
                    // http-upgrade-opts независимо от security, поэтому у
                    // открытого WS за CDN заголовок обязан приехать в `hst`,
                    // иначе узел не набирается. Равный `sni` опускается (8.2.1).
                    let host = si.sni.to_ascii_lowercase();
                    if node.sni.as_deref() != Some(host.as_str()) {
                        node.host_header = Some(host);
                    }
                }
                Network::Grpc => node.path = Some(si.grpc_service.clone()),
                _ => {}
            }
        }
        Protocol::Hysteria2 => {
            node.sni = Some(si.sni.to_ascii_lowercase());
            node.insecure = Some(true);
            node.obfs = si.hy2_obfs.clone();
        }
        Protocol::Tuic => {
            // Без `sni`: генератор Clash его для tuic не выпускает, а проверка
            // сертификата выключена, так что имени не с чем сверяться.
            node.insecure = Some(true);
            node.alpn = vec![Alpn::H3];
            node.congestion = congestion_of(si.tuic_congestion_control.as_deref());
            node.zero_rtt = si.tuic_zero_rtt_handshake.filter(|z| *z);
        }
        Protocol::Shadowsocks => {
            node.ss_method = Some(ss_method_of(&inbound.settings)?);
        }
        Protocol::Wireguard => {
            node.public_key = Some(reality_key(&si.public_key)?);
            node.mtu = Some(WIREGUARD_MTU);
        }
        Protocol::Naive => return Ok(None),
    }
    Ok(Some(node))
}

fn country_of(code: Option<&str>) -> String {
    let cc = code.unwrap_or("").trim().to_ascii_uppercase();
    if cc.len() == 2 && cc.bytes().all(|b| b.is_ascii_uppercase()) {
        cc
    } else {
        UNKNOWN_COUNTRY.to_string()
    }
}

/// Ключ Reality или wireguard хранится в base64 в одном из двух алфавитов;
/// на провод уходят сырые 32 байта.
fn reality_key(encoded: &str) -> Result<[u8; 32], &'static str> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let raw = encoded.trim();
    let decoded = [URL_SAFE_NO_PAD, URL_SAFE, STANDARD_NO_PAD, STANDARD]
        .iter()
        .find_map(|e| e.decode(raw).ok())
        .ok_or("публичный ключ не base64")?;
    decoded.try_into().map_err(|_| "публичный ключ не 32 байта")
}

fn fingerprint_of(s: &str) -> Option<Fingerprint> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "chrome" => Fingerprint::Chrome,
        "firefox" => Fingerprint::Firefox,
        "safari" => Fingerprint::Safari,
        "ios" => Fingerprint::Ios,
        "android" => Fingerprint::Android,
        "edge" => Fingerprint::Edge,
        "360" => Fingerprint::Qihoo360,
        "qq" => Fingerprint::Qq,
        "random" => Fingerprint::Random,
        "randomized" => Fingerprint::Randomized,
        _ => return None,
    })
}

/// `bbr` это значение по умолчанию и у генератора, и у словаря `cg`, поэтому
/// оно не выпускается.
fn congestion_of(s: Option<&str>) -> Option<Congestion> {
    match s.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("cubic") => Some(Congestion::Cubic),
        Some("new_reno") | Some("newreno") => Some(Congestion::NewReno),
        _ => None,
    }
}

fn ss_method_of(settings_raw: &str) -> Result<SsMethod, &'static str> {
    let v: serde_json::Value = serde_json::from_str(settings_raw).unwrap_or_default();
    let method = v
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("2022-blake3-aes-128-gcm");
    Ok(match method {
        "2022-blake3-aes-128-gcm" => SsMethod::Blake3Aes128Gcm,
        "2022-blake3-aes-256-gcm" => SsMethod::Blake3Aes256Gcm,
        "2022-blake3-chacha20-poly1305" => SsMethod::Blake3Chacha20Poly1305,
        "aes-128-gcm" => SsMethod::Aes128Gcm,
        "aes-256-gcm" => SsMethod::Aes256Gcm,
        "chacha20-ietf-poly1305" => SsMethod::Chacha20IetfPoly1305,
        _ => return Err("метод shadowsocks вне словаря ssm"),
    })
}

// ---------------------------------------------------------------- решение о подписи

/// Строка `csm_catalogs`: всё, что нужно, чтобы отдать тир и назвать его в
/// директиве, без самого кадра.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCatalog {
    pub id: i64,
    pub tier: u64,
    /// Ключ хранения, [`Catalog::storage_digest`]: дайджест содержимого,
    /// связанный с тенантом и подписантом. Колонка называется по дайджесту
    /// содержимого, потому что им и определяется, когда тир переподписывать.
    pub content_digest: [u8; 32],
    pub ver: u64,
    pub iat: u64,
    pub chash: [u8; 32],
    pub chunk_count: u64,
}

/// Что делать с моделью тира: отдать хранимый кадр или подписать новый.
#[derive(Debug)]
pub enum Decision {
    Reuse(StoredCatalog),
    Sign {
        /// Строка того же ключа хранения, которую новый кадр замещает на
        /// месте: каталог доживает срок, содержимое не менялось.
        replaces: Option<StoredCatalog>,
        content_digest: [u8; 32],
        ver: u64,
        iat: u64,
        signed: SignedCatalog,
    },
}

/// Ключ хранения строки для этого подписанта.
pub fn storage_digest(model: &Catalog, signer: &SigningKey) -> [u8; 32] {
    model.storage_digest(&csm::keyid_trunc(&signer.verifying_key().to_bytes()))
}

/// Строка доживает срок: до `exp` осталось меньше запаса.
pub fn expiring(row: &StoredCatalog, now: u64, margin: u64) -> bool {
    row.iat + LIFETIME_CATALOG <= now + margin
}

/// Решение по `03-WIRE.md` 1.5. `existing` это строка с тем же ключом
/// хранения, если она есть: совпадение означает, что содержимое не менялось и
/// подписант тот же, и подпись не выпускается, пока кадру не пора истекать.
/// Иначе `ver` берётся из счётчика тира, `iat` это `now`, момент, когда
/// панель увидела изменение или решила продлить, а набивка каталога и частей
/// вытягивается один раз этой корзиной (12.3); `None` оставляет кадр без
/// набивки, как у генератора корпуса.
pub fn decide(
    model: &mut Catalog,
    existing: Option<StoredCatalog>,
    next_ver: u64,
    now: u64,
    signer: &SigningKey,
    bucket: Option<u32>,
) -> Result<Decision, CatalogError> {
    let digest = storage_digest(model, signer);
    let margin = renew_margin(model.ttl);
    let replaces = match existing {
        Some(row) if row.content_digest == digest => {
            if !expiring(&row, now, margin) {
                return Ok(Decision::Reuse(row));
            }
            Some(row)
        }
        _ => None,
    };
    model.ver = next_ver;
    model.iat = now;
    let signers = std::slice::from_ref(signer);
    let signed = match bucket {
        Some(r) => model.sign_padded(signers, r)?,
        None => model.sign(signers)?,
    };
    Ok(Decision::Sign {
        replaces,
        content_digest: digest,
        ver: next_ver,
        iat: now,
        signed,
    })
}

// ---------------------------------------------------------------- хранилище

/// Таблицы `csm_catalogs` и `csm_catalog_chunks`.
pub struct CatalogStore<'a> {
    pool: &'a PgPool,
}

impl<'a> CatalogStore<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        CatalogStore { pool }
    }

    /// Строка тира с данным дайджестом содержимого.
    pub async fn find_by_digest(
        &self,
        tier: u64,
        content_digest: &[u8; 32],
    ) -> Result<Option<StoredCatalog>> {
        let row: Option<(i64, i64, i64, String, i64)> = sqlx::query_as(
            "SELECT k.id, k.ver, k.iat, k.chash, \
                    (SELECT COUNT(*) FROM csm_catalog_chunks c WHERE c.catalog_id = k.id) \
             FROM csm_catalogs k WHERE k.tier = $1 AND k.content_digest = $2",
        )
        .bind(tier as i32)
        .bind(hex(content_digest))
        .fetch_optional(self.pool)
        .await
        .context("csm: поиск каталога по дайджесту")?;
        row.map(|(id, ver, iat, chash, chunks)| {
            Ok(StoredCatalog {
                id,
                tier,
                content_digest: *content_digest,
                ver: ver as u64,
                iat: iat as u64,
                chash: from_hex::<32>(&chash)?,
                chunk_count: chunks as u64,
            })
        })
        .transpose()
    }

    /// Следующая версия для тира: счётчик изменений содержимого.
    pub async fn next_ver(&self, tier: u64) -> Result<u64> {
        let max: Option<i64> =
            sqlx::query_scalar("SELECT MAX(ver) FROM csm_catalogs WHERE tier = $1")
                .bind(tier as i32)
                .fetch_one(self.pool)
                .await
                .context("csm: версия каталога")?;
        Ok(max.unwrap_or(0) as u64 + 1)
    }

    /// Сохраняет подписанный каталог с частями в одной транзакции. Гонка двух
    /// процессов панели на одном дайджесте разрешается уникальным индексом:
    /// проигравший читает строку победителя. Кадры у них могут отличаться
    /// корзиной набивки, и это нормально: клиенту нужен один кадр на тир, а
    /// не один кадр на модель.
    pub async fn insert(
        &self,
        tier: u64,
        content_digest: &[u8; 32],
        ver: u64,
        iat: u64,
        signed: &SignedCatalog,
    ) -> Result<StoredCatalog> {
        let mut tx = self.pool.begin().await.context("csm: транзакция")?;
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO csm_catalogs (tier, content_digest, ver, iat, chash, frame) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (tier, content_digest) DO NOTHING RETURNING id",
        )
        .bind(tier as i32)
        .bind(hex(content_digest))
        .bind(ver as i64)
        .bind(iat as i64)
        .bind(hex(&signed.chash))
        .bind(&signed.frame)
        .fetch_optional(&mut *tx)
        .await
        .context("csm: запись каталога")?;

        let Some(id) = inserted else {
            tx.rollback().await.ok();
            return self
                .find_by_digest(tier, content_digest)
                .await?
                .ok_or_else(|| anyhow!("csm: каталог исчез между конфликтом и чтением"));
        };

        insert_chunks(&mut tx, id, signed).await?;
        tx.commit().await.context("csm: фиксация каталога")?;

        Ok(StoredCatalog {
            id,
            tier,
            content_digest: *content_digest,
            ver,
            iat,
            chash: signed.chash,
            chunk_count: signed.chunk_count() as u64,
        })
    }

    /// Замещает доживающую строку на месте: тот же ключ хранения, новые
    /// `ver`, `iat`, `chash`, кадр и части. Условие на прежний `iat` делает
    /// продление оптимистичным: второй процесс, продливший раньше, оставляет
    /// первому ноль строк, и тот читает победителя.
    pub async fn renew(
        &self,
        stale: &StoredCatalog,
        ver: u64,
        iat: u64,
        signed: &SignedCatalog,
    ) -> Result<StoredCatalog> {
        let mut tx = self.pool.begin().await.context("csm: транзакция")?;
        let updated: Option<i64> = sqlx::query_scalar(
            "UPDATE csm_catalogs SET ver = $2, iat = $3, chash = $4, frame = $5, \
                    created_at = NOW() \
             WHERE id = $1 AND iat = $6 RETURNING id",
        )
        .bind(stale.id)
        .bind(ver as i64)
        .bind(iat as i64)
        .bind(hex(&signed.chash))
        .bind(&signed.frame)
        .bind(stale.iat as i64)
        .fetch_optional(&mut *tx)
        .await
        .context("csm: продление каталога")?;

        let Some(id) = updated else {
            tx.rollback().await.ok();
            return self
                .find_by_digest(stale.tier, &stale.content_digest)
                .await?
                .ok_or_else(|| anyhow!("csm: каталог исчез между продлением и чтением"));
        };

        sqlx::query("DELETE FROM csm_catalog_chunks WHERE catalog_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("csm: удаление старых частей")?;
        insert_chunks(&mut tx, id, signed).await?;
        tx.commit().await.context("csm: фиксация продления")?;

        Ok(StoredCatalog {
            id,
            tier: stale.tier,
            content_digest: stale.content_digest,
            ver,
            iat,
            chash: signed.chash,
            chunk_count: signed.chunk_count() as u64,
        })
    }

    /// Кадр части по идентификатору каталога, индексу и тиру. Тир входит в
    /// условие намеренно: `cat_id` чужого тира обязан быть неотличим от
    /// несуществующего (`03-WIRE.md` 13.5).
    pub async fn chunk(&self, tier: u64, cid: &[u8; 10], index: u64) -> Result<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT c.frame FROM csm_catalog_chunks c \
             JOIN csm_catalogs k ON k.id = c.catalog_id \
             WHERE k.tier = $1 AND left(k.chash, 20) = $2 AND c.idx = $3",
        )
        .bind(tier as i32)
        .bind(hex(cid))
        .bind(index as i32)
        .fetch_optional(self.pool)
        .await
        .context("csm: чтение части каталога")?;
        Ok(row.map(|(f,)| f))
    }
}

async fn insert_chunks(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    catalog_id: i64,
    signed: &SignedCatalog,
) -> Result<()> {
    let total = signed.chunk_count() as i32;
    for (idx, frame) in signed.chunks.iter().enumerate() {
        sqlx::query(
            "INSERT INTO csm_catalog_chunks (catalog_id, idx, total, frame) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(catalog_id)
        .bind(idx as i32)
        .bind(total)
        .bind(frame)
        .execute(&mut **tx)
        .await
        .context("csm: запись части каталога")?;
    }
    Ok(())
}

/// Каталог тира, готовый к отдаче: хранимая строка и модель, из которой она
/// выведена. Модель нужна директиве, чтобы назвать выбранный выход по `id`.
pub struct TierCatalog {
    pub stored: StoredCatalog,
    pub model: Catalog,
}

/// Собирает модель тира, сравнивает дайджест с хранилищем и подписывает
/// только при изменении. Единственный путь, которым каталог попадает в базу.
pub async fn ensure_tier(
    state: &AppState,
    signer: &SigningKey,
    pid: [u8; 8],
    tier: u64,
) -> Result<TierCatalog> {
    let tiers = count_tiers(&state.pool).await?;
    if tiers > MAX_TIERS {
        bail!(
            "csm: у тенанта {} планов, ключевой документ вмещает {MAX_TIERS} тиров",
            tiers
        );
    }
    let mut model = load_tier_model(state, pid, tier).await?;
    let store = CatalogStore::new(&state.pool);
    let digest = storage_digest(&model, signer);
    let existing = store.find_by_digest(tier, &digest).await?;
    let now = chrono::Utc::now().timestamp() as u64;
    let needs_signing = existing
        .as_ref()
        .is_none_or(|row| expiring(row, now, renew_margin(model.ttl)));
    let next_ver = if needs_signing {
        store.next_ver(tier).await?
    } else {
        0
    };
    let bucket = Some(draw_bucket(model.pad_buckets));

    let stored = match decide(&mut model, existing, next_ver, now, signer, bucket)
        .map_err(|e| anyhow!("csm: тир {tier} тенанта {}: {e}", hex(&pid)))?
    {
        Decision::Reuse(row) => row,
        Decision::Sign {
            replaces,
            content_digest,
            ver,
            iat,
            signed,
        } => {
            if signed.exceeds_warn() {
                tracing::warn!(
                    tenant = %hex(&pid),
                    tier,
                    exits = model.exits.len(),
                    payload_len = signed.payload_len,
                    "csm: payload каталога выше PANEL_WARN"
                );
            }
            let renewed = replaces.is_some();
            let row = match replaces {
                Some(stale) => store.renew(&stale, ver, iat, &signed).await?,
                None => {
                    store
                        .insert(tier, &content_digest, ver, iat, &signed)
                        .await?
                }
            };
            tracing::info!(
                tenant = %hex(&pid),
                tier,
                ver = row.ver,
                cat_id = %signed.cat_id(),
                chunks = row.chunk_count,
                renewed,
                "csm: каталог тира переподписан"
            );
            row
        }
    };
    Ok(TierCatalog { stored, model })
}

// ---------------------------------------------------------------- выбор части

/// Индекс части из пути: десятичное число без ведущих нулей (`03-WIRE.md`
/// 13.2). `"01"` это не `1`: один и тот же ресурс обязан иметь один URL, иначе
/// кэш держит его дважды.
pub fn parse_chunk_index(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    let i: u64 = s.parse().ok()?;
    (i < caramba_shared::csm::catalog::MAX_CHUNKS as u64).then_some(i)
}

/// `cat_id` из пути: ровно 16 символов base32 Crockford, 10 байт `chash`.
pub fn parse_cat_id(s: &str) -> Option<[u8; 10]> {
    if s.len() != 16 {
        return None;
    }
    crockford_decode(s).ok()?.try_into().ok()
}

/// Сильный валидатор части (`03-WIRE.md` 13.4).
pub fn chunk_etag(cat_id: &str, index: u64) -> String {
    format!("\"{cat_id}-{index}\"")
}

/// Часть длиннее потолка ответа не отдаётся: кэш и лестница клиента считают
/// на `CHUNK_RESP_MAX`, и кадр выше него это ошибка подписи, а не ответ.
pub fn chunk_fits(frame: &[u8]) -> bool {
    frame.len() <= caramba_shared::csm::catalog::CHUNK_RESP_MAX
}

// ---------------------------------------------------------------- локаторы

/// Локатор подписки: читает строку `csm_subscriptions` или создаёт её с
/// поколением 1. Секрет приходит из окружения, поэтому строки появляются
/// лениво, а не миграцией.
pub async fn ensure_locator(
    pool: &PgPool,
    secret: &[u8; 32],
    subscription_id: i64,
    subscription_uuid: &str,
) -> Result<Locator> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT locator FROM csm_subscriptions WHERE subscription_id = $1")
            .bind(subscription_id)
            .fetch_optional(pool)
            .await
            .context("csm: чтение локатора")?;
    if let Some((loc,)) = row {
        return Locator::parse(&loc).map_err(|e| anyhow!("csm: локатор в базе испорчен: {e}"));
    }
    let loc = Locator::derive(secret, subscription_uuid, 1);
    sqlx::query(
        "INSERT INTO csm_subscriptions (subscription_id, gen, locator) VALUES ($1, 1, $2) \
         ON CONFLICT (subscription_id) DO NOTHING",
    )
    .bind(subscription_id)
    .bind(loc.as_str())
    .execute(pool)
    .await
    .context("csm: запись локатора")?;
    Ok(loc)
}

/// Момент последнего дозаполнения, секунды Unix. Дозаполнение идёт на старте
/// и при промахе локатора, не чаще раза в минуту на процесс и только одним
/// вызывающим за раз: неизвестный локатор с публичного маршрута не должен
/// превращаться в проход по подпискам на каждый запрос, тем более в N
/// параллельных проходов из N одновременных промахов.
static LAST_BACKFILL: AtomicU64 = AtomicU64::new(0);
const BACKFILL_MIN_INTERVAL: u64 = 60;

/// Создаёт локаторы подпискам, у которых их ещё нет. Возвращает число
/// добавленных строк; `None`, если дозаполнение пропущено по троттлингу.
pub async fn backfill_locators(pool: &PgPool, secret: &[u8; 32]) -> Result<Option<usize>> {
    let now = chrono::Utc::now().timestamp() as u64;
    let last = LAST_BACKFILL.load(Ordering::Relaxed);
    if now.saturating_sub(last) < BACKFILL_MIN_INTERVAL
        || LAST_BACKFILL
            .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
    {
        return Ok(None);
    }

    let missing: Vec<(i64, String)> = sqlx::query_as(
        "SELECT s.id, s.subscription_uuid FROM subscriptions s \
         LEFT JOIN csm_subscriptions c ON c.subscription_id = s.id \
         WHERE c.subscription_id IS NULL AND s.subscription_uuid IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .context("csm: подписки без локатора")?;
    let mut added = 0;
    for (id, uuid) in &missing {
        ensure_locator(pool, secret, *id, uuid).await?;
        added += 1;
    }
    Ok(Some(added))
}

/// Дозаполнение на старте панели: единственный момент, когда полный проход по
/// подпискам не стоит на пути чужого запроса. Без секрета ничего не делает.
pub async fn backfill_at_startup(pool: &PgPool) {
    let secret = match super::loc_secret_from_env() {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => {
            tracing::error!(error = %e, "csm: секрет локатора не разбирается, локаторы не выдаются");
            return;
        }
    };
    match backfill_locators(pool, &secret).await {
        Ok(Some(added)) => tracing::info!(added, "csm: локаторы дозаполнены на старте"),
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "csm: дозаполнение локаторов на старте"),
    }
}

/// Подписка, найденная по локатору, с фактами, из которых собирается
/// директива. Ничего лишнего: устройство, IP и трекинг доступа сюда не
/// входят намеренно (`03-WIRE.md` 13.3).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LocatedSubscription {
    pub id: i64,
    pub plan_id: i64,
    pub node_id: Option<i64>,
    pub status: String,
    pub used_traffic: i64,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub relay_country: Option<String>,
    pub traffic_limit_gb: i32,
    pub is_free: bool,
    pub bonus_traffic_mb: i64,
    pub banned: bool,
}

impl LocatedSubscription {
    /// Потолок трафика в байтах: план плюс разовый бонус, 0 без лимита
    /// (та же арифметика, что в `ensure_subscription_within_quota`).
    pub fn limit_bytes(&self) -> i64 {
        if self.traffic_limit_gb <= 0 {
            return 0;
        }
        self.traffic_limit_gb as i64 * 1024 * 1024 * 1024 + self.bonus_traffic_mb * 1024 * 1024
    }
}

pub async fn find_by_locator(pool: &PgPool, loc: &Locator) -> Result<Option<LocatedSubscription>> {
    sqlx::query_as::<_, LocatedSubscription>(
        "SELECT s.id, s.plan_id, s.node_id, s.status, \
                COALESCE(s.used_traffic, 0)::BIGINT AS used_traffic, \
                s.expires_at, s.relay_country, \
                COALESCE(p.traffic_limit_gb, 0)::INT AS traffic_limit_gb, \
                COALESCE(p.is_free, FALSE) AS is_free, \
                COALESCE(u.bonus_traffic_mb, 0)::BIGINT AS bonus_traffic_mb, \
                COALESCE(u.is_banned, FALSE) AS banned \
         FROM csm_subscriptions c \
         JOIN subscriptions s ON s.id = c.subscription_id \
         JOIN plans p ON p.id = s.plan_id \
         JOIN users u ON u.id = s.user_id \
         WHERE c.locator = $1",
    )
    .bind(loc.as_str())
    .fetch_optional(pool)
    .await
    .context("csm: поиск подписки по локатору")
}

/// Выделяет следующую версию директивы для локатора (`02-SPEC.md` 4.7):
/// один атомарный инкремент, общий для всех устройств подписки.
pub async fn next_directive_ver(pool: &PgPool, subscription_id: i64) -> Result<u64> {
    let ver: i64 = sqlx::query_scalar(
        "UPDATE csm_subscriptions SET directive_ver = directive_ver + 1, updated_at = NOW() \
         WHERE subscription_id = $1 RETURNING directive_ver",
    )
    .bind(subscription_id)
    .fetch_one(pool)
    .await
    .context("csm: версия директивы")?;
    Ok(ver as u64)
}

// ---------------------------------------------------------------- директива

/// Факты о подписке, от которых зависят `st` и `rc`.
#[derive(Debug, Clone)]
pub struct StatusFacts<'a> {
    pub status: &'a str,
    pub expires_at: i64,
    pub used_traffic: i64,
    /// 0 означает «без лимита».
    pub limit_bytes: i64,
    /// Бесплатный план: это onboarding-грант панели, регистрация сажает
    /// человека именно на него, и его исчерпание это исчерпание гранта.
    pub free_plan: bool,
    pub banned: bool,
    pub now: i64,
}

/// Отображение состояния подписки панели на словарь `st`/`rc`, строка в
/// строку по таблице `02-SPEC.md` 4.6.2. Отказ едет подписанным полем внутри
/// 200, а не кодом HTTP (`03-WIRE.md` 13.5).
pub fn classify(f: &StatusFacts<'_>) -> (Status, ReasonCode) {
    if f.banned {
        return (Status::Suspended, ReasonCode::ACCOUNT_SUSPENDED);
    }
    let over_quota = f.limit_bytes > 0 && f.used_traffic >= f.limit_bytes;
    match f.status {
        "pending" => (Status::PendingApproval, ReasonCode::AWAITING_APPROVAL),
        // `throttled` это суточная блокировка бесплатного плана и ничего
        // больше: код причины не зависит от того, как план настроен.
        "throttled" => (Status::QuotaExceeded, ReasonCode::DAILY_ALLOWANCE_EXHAUSTED),
        "expired" => (Status::Expired, ReasonCode::TERM_ENDED),
        "active" if f.expires_at <= f.now => (Status::Expired, ReasonCode::TERM_ENDED),
        "active" if over_quota && f.free_plan => (
            Status::QuotaExceeded,
            ReasonCode::ONBOARDING_GRANT_EXHAUSTED,
        ),
        "active" if over_quota => (Status::QuotaExceeded, ReasonCode::TRAFFIC_QUOTA_EXHAUSTED),
        "active" if f.free_plan => (Status::Onboarding, ReasonCode::NONE),
        "active" => (Status::Active, ReasonCode::NONE),
        // Любой статус, который панель ввела вне этого словаря, для клиента
        // означает «оператор остановил обслуживание».
        _ => (Status::Suspended, ReasonCode::ACCOUNT_SUSPENDED),
    }
}

impl LocatedSubscription {
    /// Факты для `classify` на момент `now`.
    pub fn status_facts(&self, now: i64) -> StatusFacts<'_> {
        StatusFacts {
            status: &self.status,
            expires_at: self.expires_at.timestamp(),
            used_traffic: self.used_traffic,
            limit_bytes: self.limit_bytes(),
            free_plan: self.is_free,
            banned: self.banned,
            now,
        }
    }

    /// Вправе ли подписка читать части каталога. Локатор переживает
    /// истечение и блокировку, а материал узлов отдаётся только тому, кто
    /// вправе подключаться: легаси `/sub/{uuid}` отдаёт конфиг только
    /// `active`, и протокол не расширяет это молча.
    pub fn may_read_chunks(&self, now: i64) -> bool {
        classify(&self.status_facts(now)).0.may_connect()
    }
}

/// Входы директивы, уже разобранные и проверенные маршрутом.
pub struct DirectiveInputs<'a> {
    pub pid: [u8; 8],
    pub ver: u64,
    pub iat: u64,
    pub nonce: Nonce,
    pub dtp: DeviceThumbprint,
    pub status: Status,
    pub reason: ReasonCode,
    pub catalog: &'a StoredCatalog,
    pub cap: u32,
    pub selection: Option<Selection>,
    pub ttl: u64,
    pub locator: Locator,
    pub traffic: Option<Traffic>,
}

/// Собирает директиву. Nonce и `dtp` берутся из запроса и никак не
/// переписываются: клиент отвергнет ответ, чей nonce не равен посланному
/// байт в байт (`02-SPEC.md` 5.3), и это единственная свежесть, которая
/// переживает неверные часы.
pub fn assemble_directive(inp: DirectiveInputs<'_>) -> Result<Directive> {
    let cap = Capabilities::from_bits(inp.cap).map_err(|e| anyhow!("csm: {e}"))?;
    Ok(Directive {
        pid: inp.pid,
        ver: inp.ver,
        iat: inp.iat,
        nonce: inp.nonce,
        dtp: inp.dtp,
        status: inp.status,
        reason: inp.reason,
        catalog: inp.catalog.chash,
        chunks: inp.catalog.chunk_count,
        tier: inp.catalog.tier,
        cap,
        selection: inp.selection,
        policy: None,
        announce: None,
        support: None,
        hints: Vec::new(),
        ttl: inp.ttl,
        grace: None,
        locator: inp.locator,
        traffic: inp.traffic,
    })
}

/// Авторитетный выбор для подписки с закреплённым узлом: первый по `id`
/// выход этого узла в каталоге. Без закрепления `sel` не выпускается, и
/// клиент выбирает сам.
pub fn selection_for(
    model: &Catalog,
    node_id: Option<i64>,
    relay_country: Option<&str>,
) -> Option<Selection> {
    let nid = node_id.filter(|n| *n > 0)?;
    let prefix = format!("n{nid}i");
    let mut ids: Vec<&str> = model
        .exits
        .iter()
        .map(|n| n.id.as_str())
        .filter(|id| id.starts_with(&prefix))
        .collect();
    ids.sort_unstable();
    let exit = ids.first()?;
    Some(Selection {
        exit: Some((*exit).to_string()),
        relay: None,
        preset: None,
        variant: 0,
        proto: None,
        rcc: relay_resolution(relay_country),
        nid: nid as u64,
    })
}

/// Сохранённый выбор релея подписки. Цепочки релэев панель пока не выпускает,
/// поэтому всё, что не двухбуквенный код, разрешается в «без релея».
pub fn relay_resolution(relay_country: Option<&str>) -> RelayResolution {
    let cc = relay_country.unwrap_or("").trim().to_ascii_uppercase();
    if cc.len() == 2 && cc.bytes().all(|b| b.is_ascii_uppercase()) {
        let b = cc.as_bytes();
        RelayResolution::Country([b[0], b[1]])
    } else {
        RelayResolution::NoRelay
    }
}

/// Корзина набивки на этот запрос: равномерно из `[pb[0], pb[1]]`.
pub fn draw_bucket(pb: [u64; 2]) -> u32 {
    use rand::Rng;
    let lo = pb[0].min(15) as u32;
    let hi = pb[1].min(15).max(pb[0]) as u32;
    rand::rng().random_range(lo..=hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use caramba_shared::csm::catalog::CHUNK_PAYLOAD_MAX;
    use caramba_shared::csm::{self, DocType};
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    // Входы корпуса (`05-TEST-VECTORS/gen/positive.go`), скопированы намеренно:
    // тест обязан упасть, если корпус пересобран с другими ключами.
    const FIX_IAT: u64 = 1_788_307_200;
    const ONLINE_SEED: &str = "3e395bd70b7b39edf135a4610ed77446cf6b964e13daa8a9eae29402de45ff57";
    const PID: &str = "226e8a20f699b964";
    const NONCE: &str = "a3f10c94b27e5d6188ff20419c73ae05";
    const DTP_LABEL: &[u8] = b"csm1-doc-example-device-spki";
    const LOC_SECRET_LABEL: &[u8] = b"csm1-doc-example-loc-secret";
    const SUB_UUID: &str = "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8";
    const LOC: &str = "EA3B8SKCY6VBWASE7AM1X48Y";
    const PUBLISHED_CAT_ID: &str = "XDE36CGS838HG4W4";
    const PUBLISHED_CHASH: &str =
        "eb5c33321940d11813848b8b8b03417e75fb36a82c8aa9c9567e1686f9df535d";

    fn corpus_dir() -> Option<PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/caramba-client/docs/protocol/05-TEST-VECTORS");
        p.exists().then_some(p)
    }

    fn fixture(name: &str) -> Option<Vec<u8>> {
        let dir = corpus_dir()?;
        Some(std::fs::read(dir.join("bin/positive").join(name)).expect(name))
    }

    fn online() -> SigningKey {
        SigningKey::from_bytes(&from_hex::<32>(ONLINE_SEED).unwrap())
    }

    fn pid() -> [u8; 8] {
        from_hex::<8>(PID).unwrap()
    }

    fn reality_pbk() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    fn node_vless_reality(id: &str, host: &str, sid: &str) -> Node {
        let mut n = Node::new(
            id,
            "\u{1F1E9}\u{1F1EA} Stealth",
            "DE",
            host,
            443,
            Protocol::Vless,
            Network::Tcp,
            Security::Reality,
        );
        n.sni = Some("www.microsoft.com".into());
        n.public_key = Some(reality_pbk());
        n.short_id = Some(sid.into());
        n.fingerprint = Some(Fingerprint::Chrome);
        n.flow = Some(Flow::Vision);
        n.insecure = Some(false);
        n
    }

    /// `catMin` генератора: тот же каталог, что воспроизводит `c1_min.bin`.
    fn minimal_model() -> Catalog {
        let mut c = tier_catalog(
            pid(),
            1,
            vec![node_vless_reality("n17i3", "de1.exa-nodes.net", "6ba85179")],
            false,
        );
        c.cap = cap::NODE_MATERIAL | cap::SEALED_DIRECTIVES;
        c
    }

    fn stored(model: &Catalog, ver: u64, iat: u64) -> (StoredCatalog, SignedCatalog) {
        let mut m = model.clone();
        m.ver = ver;
        m.iat = iat;
        let signed = m.sign(&[online()]).unwrap();
        (
            StoredCatalog {
                id: 1,
                tier: m.tier,
                content_digest: storage_digest(&m, &online()),
                ver,
                iat,
                chash: signed.chash,
                chunk_count: signed.chunk_count() as u64,
            },
            signed,
        )
    }

    #[test]
    fn signing_path_reproduces_the_corpus_catalog_and_chunk() {
        let Some(expected) = fixture("c1_min.bin") else {
            return;
        };
        let expected_chunk = fixture("c1c_min_0.bin").unwrap();
        let mut model = minimal_model();
        let d = decide(&mut model, None, 7, FIX_IAT, &online(), None).unwrap();
        let Decision::Sign {
            signed,
            ver,
            iat,
            replaces,
            ..
        } = d
        else {
            panic!("без хранимой строки каталог обязан подписаться");
        };
        assert!(replaces.is_none());
        assert_eq!((ver, iat), (7, FIX_IAT));
        assert_eq!(
            signed.frame, expected,
            "кадр каталога разошёлся с c1_min.bin"
        );
        assert_eq!(signed.frame.len(), 272);
        assert_eq!(signed.chunks.len(), 1);
        assert_eq!(
            signed.chunks[0], expected_chunk,
            "часть разошлась с c1c_min_0.bin"
        );
        assert_eq!(signed.chunks[0].len(), 407);
        assert_eq!(signed.cat_id(), PUBLISHED_CAT_ID);
        assert_eq!(hex(&signed.chash), PUBLISHED_CHASH);
    }

    #[test]
    fn same_fleet_gives_the_same_digest_and_no_resign() {
        let model = minimal_model();
        let (row, _) = stored(&model, 7, FIX_IAT);

        // Тот же флот в другом порядке строк и в другую секунду.
        let mut again = model.clone();
        again
            .exits
            .push(node_vless_reality("n1i1", "de2.exa-nodes.net", "1f2e3d4c"));
        let mut base_two = again.clone();
        base_two.exits.reverse();
        assert_eq!(again.content_digest(), base_two.content_digest());

        let mut later = model.clone();
        match decide(
            &mut later,
            Some(row.clone()),
            99,
            FIX_IAT + 86_400,
            &online(),
            Some(2),
        )
        .unwrap()
        {
            Decision::Reuse(r) => {
                assert_eq!(r, row);
                assert_eq!(
                    later.ver, 0,
                    "модель не трогается при повторном использовании"
                );
            }
            Decision::Sign { .. } => panic!("неизменённый флот переподписан"),
        }
    }

    #[test]
    fn another_signer_forces_one_resign_of_the_same_fleet() {
        let model = minimal_model();
        let (row, _) = stored(&model, 7, FIX_IAT);
        let rotated = SigningKey::from_bytes(&[0x42; 32]);
        assert_ne!(
            storage_digest(&model, &online()),
            storage_digest(&model, &rotated)
        );
        // Маршрут ищет строку по ключу хранения нового подписанта и не находит
        // её; но даже строка старого подписанта, поданная напрямую, отвергается.
        let mut m = model.clone();
        match decide(&mut m, Some(row), 8, FIX_IAT + 60, &rotated, None).unwrap() {
            Decision::Sign { replaces, .. } => assert!(replaces.is_none()),
            Decision::Reuse(_) => panic!("кадр под отозванным kid переиспользован"),
        }
    }

    #[test]
    fn an_expiring_catalog_is_renewed_in_place_before_it_expires() {
        let model = minimal_model();
        let (row, first) = stored(&model, 7, FIX_IAT);
        let margin = renew_margin(model.ttl);
        assert!(margin >= 2 * TIER_TTL + 300);

        // За запас до истечения строка ещё живёт.
        let live = FIX_IAT + LIFETIME_CATALOG - margin - 1;
        assert!(!expiring(&row, live, margin));
        let mut m = model.clone();
        assert!(matches!(
            decide(&mut m, Some(row.clone()), 8, live, &online(), None).unwrap(),
            Decision::Reuse(_)
        ));

        // На границе запаса тир переподписывается с новой версией и тем же
        // ключом хранения, замещая строку на месте.
        let due = FIX_IAT + LIFETIME_CATALOG - margin;
        assert!(expiring(&row, due, margin));
        let mut m = model.clone();
        match decide(&mut m, Some(row.clone()), 8, due, &online(), None).unwrap() {
            Decision::Sign {
                replaces,
                content_digest,
                ver,
                iat,
                signed,
            } => {
                assert_eq!(replaces, Some(row.clone()));
                assert_eq!(content_digest, row.content_digest);
                assert_eq!((ver, iat), (8, due));
                assert_ne!(signed.chash, first.chash);
            }
            Decision::Reuse(_) => panic!("истекающий каталог не продлён"),
        }
        // Строка на минуту старше срока жизни тоже продлевается, не отдаётся.
        let mut m = model.clone();
        assert!(matches!(
            decide(
                &mut m,
                Some(row),
                8,
                FIX_IAT + LIFETIME_CATALOG + 60,
                &online(),
                None
            )
            .unwrap(),
            Decision::Sign {
                replaces: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn the_panel_path_pads_catalog_and_chunks_once_at_signing() {
        let mut big = minimal_model();
        big.exits = (0..40)
            .map(|i| {
                node_vless_reality(
                    &format!("n{}i{}", 100 + i, 1 + i % 7),
                    "de1.exa-nodes.net",
                    "6ba85179",
                )
            })
            .collect();
        let Decision::Sign { signed, .. } =
            decide(&mut big, None, 1, FIX_IAT, &online(), Some(3)).unwrap()
        else {
            panic!("нет строки, обязан подписаться");
        };
        assert_eq!(signed.frame.len() % 256, 0);
        assert!(signed.chunk_count() >= 2);
        for c in &signed.chunks {
            assert_eq!(c.len() % 256, 0, "часть вне сетки");
            assert!(chunk_fits(c), "часть выше CHUNK_RESP_MAX");
        }
        // r = 3 зажимается под 3584 для полной части.
        assert_eq!(signed.chunks[0].len(), 3584);
    }

    #[test]
    fn a_changed_node_changes_the_digest_and_resigns_with_the_next_version() {
        let model = minimal_model();
        let (row, first) = stored(&model, 7, FIX_IAT);

        let mut changed = model.clone();
        changed.exits[0].port = 8443;
        assert_ne!(changed.content_digest(), row.content_digest);

        // Строка с тем же дайджестом отсутствует: маршрут передаёт None.
        match decide(&mut changed, None, 8, FIX_IAT + 60, &online(), None).unwrap() {
            Decision::Sign {
                content_digest,
                ver,
                iat,
                signed,
                ..
            } => {
                assert_eq!(content_digest, storage_digest(&changed, &online()));
                assert_eq!((ver, iat), (8, FIX_IAT + 60));
                assert_ne!(signed.chash, first.chash);
                assert_ne!(signed.cat_id(), first.cat_id());
            }
            Decision::Reuse(_) => panic!("изменённый узел не переподписан"),
        }

        // Ту же модель с той же строкой решение отвергает даже при равном ver.
        let mut same = model.clone();
        let stale = StoredCatalog {
            content_digest: [0u8; 32],
            ..row
        };
        assert!(matches!(
            decide(&mut same, Some(stale), 8, FIX_IAT, &online(), None).unwrap(),
            Decision::Sign { .. }
        ));
    }

    #[test]
    fn chunk_selection_and_identifiers() {
        assert_eq!(parse_chunk_index("0"), Some(0));
        assert_eq!(parse_chunk_index("7"), Some(7));
        assert_eq!(parse_chunk_index("63"), Some(63));
        assert_eq!(parse_chunk_index("64"), None, "n <= 64, индекс < 64");
        assert_eq!(parse_chunk_index("01"), None, "ведущий ноль это другой URL");
        assert_eq!(parse_chunk_index(""), None);
        assert_eq!(parse_chunk_index("-1"), None);
        assert_eq!(parse_chunk_index("1a"), None);

        let cid = parse_cat_id(PUBLISHED_CAT_ID).expect("cat_id корпуса разбирается");
        assert_eq!(hex(&cid), &PUBLISHED_CHASH[..20], "cid это chash[0..10]");
        assert!(parse_cat_id("XDE36CGS838HG4W").is_none(), "15 символов");
        assert!(parse_cat_id("XDE36CGS838HG4W4Z").is_none(), "17 символов");
        assert!(parse_cat_id("XDE36CGS838HG4W!").is_none());
        // Строчное написание разрешено читателю Crockford и даёт те же байты.
        assert_eq!(
            parse_cat_id(&PUBLISHED_CAT_ID.to_ascii_lowercase()),
            Some(cid)
        );

        assert_eq!(chunk_etag(PUBLISHED_CAT_ID, 3), "\"XDE36CGS838HG4W4-3\"");

        // Кадр на 40 выходов режется на несколько частей, каждая под потолком.
        let mut big = minimal_model();
        big.exits = (0..40)
            .map(|i| {
                node_vless_reality(
                    &format!("n{}i{}", 100 + i, 1 + i % 7),
                    "de1.exa-nodes.net",
                    "6ba85179",
                )
            })
            .collect();
        let signed = big.sign(&[online()]).unwrap();
        assert!(signed.chunk_count() >= 2);
        assert_eq!(
            signed.frame.len().div_ceil(CHUNK_PAYLOAD_MAX),
            signed.chunk_count()
        );
        assert!(signed.chunks.iter().all(|c| chunk_fits(c)));
        let tl = signed.frame.len();
        let joined: Vec<u8> = signed
            .chunks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                // Срез кадра это последнее поле payload части (ключ 14), его
                // длина известна из правила нарезки 8.4.
                let data_len = (tl - i * CHUNK_PAYLOAD_MAX).min(CHUNK_PAYLOAD_MAX);
                let payload_len = u16::from_be_bytes([c[5], c[6]]) as usize;
                let payload = &c[7..7 + payload_len];
                payload[payload.len() - data_len..].to_vec()
            })
            .collect::<Vec<Vec<u8>>>()
            .concat();
        assert_eq!(joined, signed.frame, "части склеиваются в исходный кадр");
    }

    fn fixture_directive(catalog: &StoredCatalog, nonce: Nonce, ver: u64) -> Directive {
        let secret = Sha256::digest(LOC_SECRET_LABEL);
        assemble_directive(DirectiveInputs {
            pid: pid(),
            ver,
            iat: FIX_IAT,
            nonce,
            dtp: DeviceThumbprint::of_spki(DTP_LABEL),
            status: Status::Active,
            reason: ReasonCode::NONE,
            catalog,
            cap: cap::NODE_MATERIAL | cap::SEALED_DIRECTIVES,
            selection: None,
            ttl: TIER_TTL,
            locator: Locator::derive(&secret, SUB_UUID, 1),
            traffic: None,
        })
        .unwrap()
    }

    #[test]
    fn directive_assembly_reproduces_the_corpus_and_binds_the_nonce() {
        let Some(expected) = fixture("m1_min.bin") else {
            return;
        };
        let (row, _) = stored(&minimal_model(), 7, FIX_IAT);
        assert_eq!(hex(&row.chash), PUBLISHED_CHASH);

        let nonce = Nonce(from_hex::<16>(NONCE).unwrap());
        let d = fixture_directive(&row, nonce, 412);
        assert_eq!(d.locator.as_str(), LOC);
        let payload = d.encode().unwrap();
        let frame = csm::build(DocType::Directive, &payload, &[online()]).unwrap();
        assert_eq!(frame, expected, "директива разошлась с m1_min.bin");
        assert_eq!(frame.len(), 228);

        // Nonce из запроса уходит в payload байт в байт и меняет кадр целиком.
        let q = nonce.to_query();
        assert_eq!(Nonce::from_query(&q).unwrap(), nonce);
        assert!(payload.windows(16).any(|w| w == nonce.0));
        let other = Nonce([0x5a; 16]);
        let other_payload = fixture_directive(&row, other, 412).encode().unwrap();
        assert_ne!(other_payload, payload);
        assert!(other_payload.windows(16).any(|w| w == other.0));
        assert!(!other_payload.windows(16).any(|w| w == nonce.0));

        // Набивка на сетке 256 и под потолком внутренней директивы.
        for bucket in 0..=3u32 {
            let padded = d.sign(&[online()], bucket).unwrap();
            assert_eq!(padded.len() % 256, 0);
            assert!(padded.len() <= caramba_shared::csm::directive::INNER_DIRECTIVE_MAX);
        }
    }

    #[test]
    fn status_classification_covers_the_panel_states() {
        let base = StatusFacts {
            status: "active",
            expires_at: FIX_IAT as i64 + 86_400,
            used_traffic: 10,
            limit_bytes: 100,
            free_plan: false,
            banned: false,
            now: FIX_IAT as i64,
        };
        let c = |f: StatusFacts<'static>| classify(&f);
        assert_eq!(c(base.clone()), (Status::Active, ReasonCode::NONE));
        assert_eq!(
            c(StatusFacts {
                expires_at: FIX_IAT as i64 - 1,
                ..base.clone()
            }),
            (Status::Expired, ReasonCode::TERM_ENDED)
        );
        assert_eq!(
            c(StatusFacts {
                used_traffic: 100,
                ..base.clone()
            }),
            (Status::QuotaExceeded, ReasonCode::TRAFFIC_QUOTA_EXHAUSTED)
        );
        assert_eq!(
            c(StatusFacts {
                used_traffic: 100,
                limit_bytes: 0,
                ..base.clone()
            }),
            (Status::Active, ReasonCode::NONE),
            "0 это без лимита"
        );
        // Бесплатный план это onboarding-грант: st = 2, исчерпание это 3002.
        assert_eq!(
            c(StatusFacts {
                free_plan: true,
                ..base.clone()
            }),
            (Status::Onboarding, ReasonCode::NONE)
        );
        assert!(Status::Onboarding.may_connect());
        assert_eq!(
            c(StatusFacts {
                free_plan: true,
                used_traffic: 100,
                ..base.clone()
            }),
            (
                Status::QuotaExceeded,
                ReasonCode::ONBOARDING_GRANT_EXHAUSTED
            )
        );
        // `throttled` это 3003 независимо от настроек плана (4.6.2).
        for free_plan in [true, false] {
            assert_eq!(
                c(StatusFacts {
                    status: "throttled",
                    free_plan,
                    ..base.clone()
                }),
                (Status::QuotaExceeded, ReasonCode::DAILY_ALLOWANCE_EXHAUSTED)
            );
        }
        assert_eq!(
            c(StatusFacts {
                status: "pending",
                ..base.clone()
            }),
            (Status::PendingApproval, ReasonCode::AWAITING_APPROVAL)
        );
        // `expired` это 4/2001 даже при исчерпанной квоте: строка таблицы одна.
        assert_eq!(
            c(StatusFacts {
                status: "expired",
                used_traffic: 100,
                ..base.clone()
            }),
            (Status::Expired, ReasonCode::TERM_ENDED)
        );
        assert_eq!(
            c(StatusFacts {
                banned: true,
                ..base.clone()
            }),
            (Status::Suspended, ReasonCode::ACCOUNT_SUSPENDED)
        );
        assert_eq!(
            c(StatusFacts {
                status: "disabled",
                ..base.clone()
            }),
            (Status::Suspended, ReasonCode::ACCOUNT_SUSPENDED)
        );
        assert!(
            !c(StatusFacts {
                status: "expired",
                ..base
            })
            .0
            .may_connect()
        );
    }

    fn located(status: &str, expires_in: i64) -> LocatedSubscription {
        LocatedSubscription {
            id: 1,
            plan_id: 1,
            node_id: None,
            status: status.into(),
            used_traffic: 0,
            expires_at: chrono::DateTime::from_timestamp(FIX_IAT as i64 + expires_in, 0).unwrap(),
            relay_country: None,
            traffic_limit_gb: 10,
            is_free: false,
            bonus_traffic_mb: 0,
            banned: false,
        }
    }

    #[test]
    fn chunks_are_read_only_by_a_subscription_that_may_connect() {
        let now = FIX_IAT as i64;
        assert!(located("active", 3600).may_read_chunks(now));
        assert!(!located("active", -1).may_read_chunks(now));
        assert!(!located("expired", 3600).may_read_chunks(now));
        assert!(!located("pending", 3600).may_read_chunks(now));
        assert!(!located("throttled", 3600).may_read_chunks(now));
        let mut banned = located("active", 3600);
        banned.banned = true;
        assert!(!banned.may_read_chunks(now));
        let mut over = located("active", 3600);
        over.used_traffic = over.limit_bytes();
        assert!(!over.may_read_chunks(now));
        let mut free = located("active", 3600);
        free.is_free = true;
        assert!(free.may_read_chunks(now), "onboarding подключается");
    }

    #[test]
    fn selection_names_the_pinned_exit_only() {
        let mut model = minimal_model();
        model
            .exits
            .push(node_vless_reality("n17i1", "de2.exa-nodes.net", "1f2e3d4c"));
        model.exits.push(node_vless_reality(
            "n170i1",
            "de3.exa-nodes.net",
            "1f2e3d4c",
        ));
        assert!(selection_for(&model, None, None).is_none());
        assert!(selection_for(&model, Some(99), None).is_none());
        let sel = selection_for(&model, Some(17), Some("ru")).unwrap();
        assert_eq!(
            sel.exit.as_deref(),
            Some("n17i1"),
            "префикс n17i не ловит n170i"
        );
        assert_eq!(sel.nid, 17);
        assert_eq!(sel.rcc, RelayResolution::Country(*b"RU"));
        assert_eq!(relay_resolution(Some("none")), RelayResolution::NoRelay);
        assert_eq!(relay_resolution(None), RelayResolution::NoRelay);
    }

    #[test]
    fn tier_and_capabilities_are_honest() {
        assert_eq!(tier_of_plan(1).unwrap(), 1);
        assert_eq!(tier_of_plan(1023).unwrap(), 1023);
        assert!(tier_of_plan(0).is_err());
        assert!(tier_of_plan(1024).is_err());
        assert!(tier_of_plan(-3).is_err());
        assert_eq!(implemented_capabilities(0, false), 0);
        assert_eq!(implemented_capabilities(3, false), cap::NODE_MATERIAL);
        assert_eq!(
            implemented_capabilities(3, false) & cap::SEALED_DIRECTIVES,
            0
        );
        assert_eq!(
            implemented_capabilities(3, true),
            cap::NODE_MATERIAL | cap::ONBOARDING_GRANT
        );
        assert_eq!(
            tier_catalog(pid(), 1, Vec::new(), true).cap,
            cap::ONBOARDING_GRANT
        );
        for _ in 0..64 {
            let b = draw_bucket(PAD_BUCKETS);
            assert!((PAD_BUCKETS[0] as u32..=PAD_BUCKETS[1] as u32).contains(&b));
        }
    }

    fn node_info(inbounds: Vec<caramba_db::models::network::Inbound>) -> NodeInfo {
        NodeInfo {
            name: "de1".into(),
            address: "198.51.100.7".into(),
            reality_port: Some(443),
            reality_sni: Some("www.microsoft.com".into()),
            reality_public_key: Some(
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(reality_pbk()),
            ),
            reality_short_id: Some("6ba85179".into()),
            hy2_port: None,
            hy2_sni: None,
            frontend_url: Some("De1.Exa-Nodes.net".into()),
            inbounds,
            relay_info: None,
            country_code: Some("de".into()),
            is_relay: false,
            config_block_ads: false,
            config_block_porn: false,
            config_block_torrent: false,
        }
    }

    fn inbound(
        id: i64,
        protocol: &str,
        port: i64,
        stream: &str,
    ) -> caramba_db::models::network::Inbound {
        caramba_db::models::network::Inbound {
            id,
            node_id: 17,
            tag: format!("in{id}"),
            protocol: protocol.into(),
            listen_port: port,
            listen_ip: "::".into(),
            settings: "{}".into(),
            stream_settings: stream.into(),
            remark: None,
            enable: true,
            renew_interval_mins: 0,
            port_range_start: 0,
            port_range_end: 0,
            last_rotated_at: None,
            created_at: None,
        }
    }

    #[test]
    fn node_entries_follow_the_clash_generator() {
        let info = node_info(vec![
            inbound(3, "vless", 443, r#"{"network":"tcp","security":"reality"}"#),
            inbound(4, "hysteria2", 8443, r#"{"network":"udp"}"#),
            inbound(5, "vless", 2053, r#"{"network":"xhttp","security":"tls"}"#),
            inbound(
                6,
                "vless",
                80,
                r#"{"network":"ws","wsSettings":{"path":"/ws"}}"#,
            ),
            inbound(7, "tuic", 8444, "{}"),
            inbound(
                8,
                "trojan",
                443,
                r#"{"network":"ws","security":"tls","wsSettings":{"path":"/t"}}"#,
            ),
        ]);
        let entries = node_entries(&info, pid(), 1);
        let ids: Vec<&str> = entries.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(
            ids,
            ["n17i3", "n17i4", "n17i6", "n17i7", "n17i8"],
            "xhttp пропущен, как у Clash"
        );

        let reality = &entries[0];
        assert_eq!(reality.pn, "\u{1F1E9}\u{1F1EA} Stealth");
        assert_eq!(reality.cc, "DE");
        assert_eq!(
            reality.host, "de1.exa-nodes.net",
            "frontend_url в нижнем регистре"
        );
        assert_eq!(reality.public_key, Some(reality_pbk()));
        assert_eq!(reality.short_id.as_deref(), Some("6ba85179"));
        assert_eq!(reality.flow, Some(Flow::Vision));
        assert_eq!(reality.fingerprint, Some(Fingerprint::Chrome));
        assert_eq!(reality.insecure, Some(false));
        assert_eq!(reality.sni.as_deref(), Some("www.microsoft.com"));

        let hy2 = &entries[1];
        assert_eq!(hy2.pn, "\u{1F1E9}\u{1F1EA} Speed");
        assert_eq!(hy2.protocol, Protocol::Hysteria2);
        assert_eq!(hy2.network, Network::Quic);
        assert_eq!(hy2.security, Security::Tls);
        assert_eq!(hy2.insecure, Some(true));

        let ws = &entries[2];
        assert_eq!(ws.pn, "\u{1F1E9}\u{1F1EA} WebSocket");
        assert_eq!(ws.network, Network::Ws);
        assert_eq!(ws.security, Security::None);
        assert_eq!(ws.path.as_deref(), Some("/ws"));
        assert!(ws.sni.is_none() && ws.flow.is_none());
        // Открытый WS: генератор Clash шлёт `Host: sni`, а sni у инбаунда без
        // своего берётся с узла, как и в разборе stream_settings генератора.
        assert_eq!(ws.host_header.as_deref(), Some("www.microsoft.com"));

        // tuic: QUIC при пустых stream_settings, без sni, как у Clash.
        let tuic = &entries[3];
        assert_eq!(tuic.protocol, Protocol::Tuic);
        assert_eq!(tuic.network, Network::Quic);
        assert!(tuic.sni.is_none());
        assert_eq!(tuic.alpn, vec![Alpn::H3]);

        // trojan + TLS + WS: Host равен sni и опускается, отпечаток только
        // под Reality.
        let trojan = &entries[4];
        assert_eq!(trojan.sni.as_deref(), Some("www.microsoft.com"));
        assert!(trojan.host_header.is_none());
        assert!(trojan.fingerprint.is_none());
        assert_eq!(trojan.path.as_deref(), Some("/t"));

        // Каталог из этих записей проходит проверку и подписывается.
        let signed = tier_catalog(pid(), 1, entries, false)
            .sign(&[online()])
            .unwrap();
        assert_eq!(signed.chunk_count(), 1);

        // Узел без страны получает ZZ, а не падает.
        let mut nameless = node_info(vec![inbound(3, "vless", 443, "{}")]);
        nameless.country_code = None;
        assert_eq!(node_entries(&nameless, pid(), 1)[0].cc, "ZZ");
    }
}

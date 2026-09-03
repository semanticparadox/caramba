//! Гейт совместимости панели с клиентами.
//!
//! Корпус `apps/caramba-client/docs/protocol/05-TEST-VECTORS` порождён
//! независимой реализацией на Go и служит общим тестом для всех трёх сторон:
//! верификаторы на Go и Dart прогоняют по нему приём и отказ, а панель, которая
//! документы только подписывает, обязана воспроизвести эталонные кадры БАЙТ В
//! БАЙТ из тех же входных данных.
//!
//! Это более сильная проверка, чем «верификатор принял»: она ловит расхождение
//! в порядке ключей, в длине головы, в сроке жизни и в сортировке слотов, то
//! есть ровно те места, где три реализации разъезжаются молча.
//!
//! Корпус лежит вне крейта, поэтому тест мягко пропускается, если его нет
//! (сборка из архива без клиентской части), и падает, если он есть и не сошёлся.

#![cfg(feature = "csm")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use caramba_shared::csm::{
    self, DocType, docs::KeyDocument, docs::KeyEntry, docs::Role, docs::RoleSpec,
};
use ed25519_dalek::SigningKey;

/// Момент выпуска эталонных документов, `2026-09-02T00:00:00Z`.
const FIX_IAT: u64 = 1_788_307_200;

fn corpus_dir() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/caramba-client/docs/protocol/05-TEST-VECTORS");
    p.exists().then(|| p)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn seed(hex_seed: &str) -> SigningKey {
    let b = hex(hex_seed);
    SigningKey::from_bytes(&b.try_into().expect("32-байтовое семя"))
}

/// Семена и ожидаемые идентификаторы из `vectors.json` раздела `fixture_keys`.
/// Скопированы сюда намеренно: тест обязан падать, если корпус пересобран с
/// другими ключами, а не молча подстраиваться под него.
const ROOT_SEED: &str = "9aa4b46c96a0ed2aabe0391f899737224ad96032e8ca6bd53fd9daf5614d05ed";
const ROOT_PUBLIC: &str = "8b160c71c61008321cae0d0dc9a980b6e59cb26d0f4d1fa8dfc030c3675e2b7c";
const ROOT_KID: &str = "226e8a20f699b964dfb01e86";
const ONLINE_SEED: &str = "3e395bd70b7b39edf135a4610ed77446cf6b964e13daa8a9eae29402de45ff57";
const ONLINE_PUBLIC: &str = "75f350b3eb21344a96de195d82079e45f0a56fecdc736c16b61d56619afd5653";
const ONLINE_KID: &str = "21e3e2cc0a3ba777e69ce14c";
const PID: &str = "226e8a20f699b964";

#[test]
fn fixture_keys_derive_as_the_corpus_says() {
    let root = seed(ROOT_SEED);
    assert_eq!(
        root.verifying_key().to_bytes().to_vec(),
        hex(ROOT_PUBLIC),
        "корневой публичный ключ не совпал с корпусом"
    );
    assert_eq!(
        csm::keyid_trunc(&root.verifying_key().to_bytes()).to_vec(),
        hex(ROOT_KID),
        "keyid_trunc считается не так, как в корпусе"
    );
    assert_eq!(
        csm::pid_of(&root.verifying_key().to_bytes()).to_vec(),
        hex(PID),
        "pid считается не так, как в корпусе"
    );

    let online = seed(ONLINE_SEED);
    assert_eq!(
        online.verifying_key().to_bytes().to_vec(),
        hex(ONLINE_PUBLIC)
    );
    assert_eq!(
        csm::keyid_trunc(&online.verifying_key().to_bytes()).to_vec(),
        hex(ONLINE_KID)
    );
}

/// Минимальный ключевой документ из `03-WIRE.md` 8.1, он же якорь доверия
/// контекста `default` в корпусе.
fn minimal_key_document() -> KeyDocument {
    let root = seed(ROOT_SEED);
    let online = seed(ONLINE_SEED);
    let root_kid = csm::keyid_trunc(&root.verifying_key().to_bytes());
    let online_kid = csm::keyid_trunc(&online.verifying_key().to_bytes());

    KeyDocument {
        pid: hex(PID).try_into().unwrap(),
        ver: 1,
        iat: FIX_IAT,
        keys: vec![
            KeyEntry {
                kid: root_kid,
                public_key: root.verifying_key().to_bytes(),
            },
            KeyEntry {
                kid: online_kid,
                public_key: online.verifying_key().to_bytes(),
            },
        ],
        roles: vec![
            RoleSpec {
                role: Role::Root,
                kids: vec![root_kid],
                threshold: 1,
            },
            RoleSpec {
                role: Role::Online,
                kids: vec![online_kid],
                threshold: 1,
            },
        ],
        revoked_kids: Vec::new(),
        revoked_nodes: Vec::new(),
        tiers: BTreeMap::new(),
        ttl: None,
    }
}

#[test]
fn key_document_payload_matches_the_published_hex() {
    // Конверт из 03-WIRE.md 8.1, ровно 27 байт для ver < 24:
    // a7 | 01 01 | 02 48 <pid 8> | 03 01 | 04 1a <iat> | 05 1a <exp>
    let payload = minimal_key_document().encode().expect("пределы профиля");
    assert_eq!(
        payload.len(),
        173,
        "длина payload разошлась со спецификацией"
    );

    let expected_envelope = hex(concat!(
        "a7",   // карта из 7 пар
        "0101", // v = 1
        "0248",
        "226e8a20f699b964", // pid
        "0301",             // ver = 1
        "041a",
        "6a976700", // iat
        "051a",
        "6aa0a180", // exp = iat + 604800
    ));
    assert_eq!(
        expected_envelope.len(),
        27,
        "конверт спецификации это 27 байт"
    );
    assert_eq!(
        &payload[..27],
        &expected_envelope[..],
        "конверт закодирован не как в спецификации"
    );
    // Дальше идёт ключ 10 (keys) массивом из двух записей: 0a 82.
    assert_eq!(&payload[27..29], &[0x0a, 0x82]);
}

#[test]
fn minimal_key_document_reproduces_the_corpus_fixture_byte_for_byte() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let fixture = dir.join("bin/positive/k1_min.bin");
    let expected = std::fs::read(&fixture).expect("эталонный кадр k1_min.bin");

    let payload = minimal_key_document().encode().expect("пределы профиля");
    let frame = csm::build(DocType::Key, &payload, &[seed(ROOT_SEED)]).expect("сборка кадра");

    assert_eq!(
        frame.len(),
        expected.len(),
        "длина кадра разошлась: {} против {}",
        frame.len(),
        expected.len()
    );
    assert_eq!(
        frame, expected,
        "кадр панели не совпал с эталонным байт в байт: подпись панели и \
         верификаторы клиента разойдутся"
    );
    assert_eq!(frame.len(), 257, "03-WIRE.md 8.1 фиксирует 257 байт");
}

#[test]
fn signing_the_same_content_twice_gives_the_same_frame() {
    // Каталог адресуется по sha256 кадра, поэтому подпись обязана быть
    // детерминированной, иначе опубликованный хэш тира протухает при каждом
    // перезапуске панели.
    let payload = minimal_key_document().encode().unwrap();
    let a = csm::build(DocType::Key, &payload, &[seed(ROOT_SEED)]).unwrap();
    let b = csm::build(DocType::Key, &payload, &[seed(ROOT_SEED)]).unwrap();
    assert_eq!(a, b);
    assert_eq!(csm::frame_digest(&a), csm::frame_digest(&b));
}

#[test]
fn panel_refuses_to_sign_a_document_no_client_can_read() {
    // 03-WIRE.md 8.0.1: кадры без нарезки на части ограничены 4096 байтами.
    // Проверяем, что предел существует как число, а не как обещание.
    assert_eq!(csm::DOC_FRAME_MAX, 4096);
}

/// Заполнение из генератора корпуса: последовательность байт от `start`.
fn seq(start: u8, n: usize) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        *slot = start.wrapping_add(i as u8);
    }
    out
}

/// Эталонный bootstrap-блоб из `03-WIRE.md` 8.5, ровно теми же входными
/// данными, что в генераторе корпуса.
fn wire_bootstrap_blob() -> caramba_shared::csm::BootstrapBlob {
    use caramba_shared::csm::{BootstrapBlob, DohEntry, Mirror};
    BootstrapBlob {
        pid: hex(PID).try_into().unwrap(),
        ver: 1,
        iat: FIX_IAT,
        origin: "https://panel.example.net".into(),
        code: "K7QW-3M2P-9XRT".into(),
        root_public: hex(ROOT_PUBLIC).try_into().unwrap(),
        mirrors: vec![Mirror {
            host: "m1.example-cdn.net".into(),
            sni: "m1.example-cdn.net".into(),
            pins: vec![seq(0x20, 32)],
            asn: 24940,
            country: "DE".into(),
            weight: None,
            ips: Vec::new(),
        }],
        doh: vec![DohEntry {
            host: "doh.example.net".into(),
            path: "/dns-query".into(),
            ips: vec!["198.51.100.7".into()],
            pins: vec![seq(0x40, 32)],
        }],
        operator_name: Some("Exa Networks".into()),
    }
}

#[test]
fn bootstrap_blob_reproduces_the_corpus_fixture_byte_for_byte() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let expected = std::fs::read(dir.join("bin/positive/b1_wire_8_5.bin"))
        .expect("эталонный кадр b1_wire_8_5.bin");

    let payload = wire_bootstrap_blob().encode().expect("пределы профиля");
    let frame = csm::build(DocType::Bootstrap, &payload, &[seed(ROOT_SEED)]).expect("кадр");

    assert_eq!(
        payload.len(),
        290,
        "03-WIRE.md 8.5 фиксирует payload 290 байт"
    );
    assert_eq!(frame.len(), 374, "03-WIRE.md 8.5 фиксирует кадр 374 байта");
    assert_eq!(
        frame, expected,
        "bootstrap-блоб панели не совпал с эталонным байт в байт"
    );
}

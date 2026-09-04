//! Гейт каталога: панель обязана воспроизвести эталонные кадры корпуса
//! `c1_min.bin` (каталог, 272 байта) и `c1c_min_0.bin` (часть 0 из 1, 407
//! байт) БАЙТ В БАЙТ из тех же входных данных, что генератор корпуса
//! (`05-TEST-VECTORS/gen/positive.go`, `buildPositives`).
//!
//! Расхождение здесь это панель и клиенты, не согласные о байтах: клиентские
//! верификаторы (Go и Dart) сверены с тем же корпусом, и подпись, которую они
//! не примут, хуже отсутствия подписи.
//!
//! Корпус лежит вне крейта, поэтому тест мягко пропускается, если его нет, и
//! падает, если он есть и не сошёлся.

#![cfg(feature = "csm")]

use std::path::{Path, PathBuf};

use caramba_shared::csm::catalog::{
    self, CHUNK_PAYLOAD_MAX, Catalog, Fingerprint, Flow, Network, Node, Protocol, Security,
    SsMethod, Thresholds, cap,
};
use caramba_shared::csm::directive::{RelayResolution, Selection};
use caramba_shared::csm::{self, DocType};
use ed25519_dalek::SigningKey;

/// Момент выпуска эталонных документов, `2026-09-02T00:00:00Z`.
const FIX_IAT: u64 = 1_788_307_200;
/// `fixCatalogV` генератора.
const FIX_CATALOG_VER: u64 = 7;
const FIX_TIER: u64 = 1;
const FIX_TTL: u64 = 7200;
const FIX_JIT: u64 = 20;

/// Семена из `vectors.json`, раздел `fixture_keys`. Скопированы намеренно:
/// тест обязан падать, если корпус пересобран с другими ключами.
const ONLINE_SEED: &str = "3e395bd70b7b39edf135a4610ed77446cf6b964e13daa8a9eae29402de45ff57";
const PID: &str = "226e8a20f699b964";

/// Опубликованные в `03-WIRE.md` 8.2 и 8.4 хэши кадров и cat_id.
const PUBLISHED_CHASH: &str = "eb5c33321940d11813848b8b8b03417e75fb36a82c8aa9c9567e1686f9df535d";
const PUBLISHED_CHUNK_SHA: &str =
    "68d613af7e4f616464ad281a92739822361fa66600948cda6ede452b46237168";
const PUBLISHED_CAT_ID: &str = "XDE36CGS838HG4W4";

fn corpus_dir() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/caramba-client/docs/protocol/05-TEST-VECTORS");
    p.exists().then_some(p)
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn online() -> SigningKey {
    SigningKey::from_bytes(&hex(ONLINE_SEED).try_into().expect("32-байтовое семя"))
}

/// `realityPBK` генератора: байты 0x00..0x1f.
fn reality_pbk() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = i as u8;
    }
    k
}

/// `nodeVlessReality` генератора: VLESS + Reality + TCP, доминирующий выход.
/// `insV: true` там означает явное `ins = false`, здесь `Some(false)`.
fn node_vless_reality(id: &str, pn: &str, cc: &str, host: &str, port: u16, sid: &str) -> Node {
    let mut n = Node::new(
        id,
        pn,
        cc,
        host,
        port,
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

/// Минимальный каталог `03-WIRE.md` 8.2, входные данные `catMin` генератора.
fn minimal_catalog() -> Catalog {
    Catalog {
        pid: hex(PID).try_into().unwrap(),
        ver: FIX_CATALOG_VER,
        iat: FIX_IAT,
        tier: FIX_TIER,
        exits: vec![node_vless_reality(
            "n17i3",
            "\u{1F1E9}\u{1F1EA} Stealth",
            "DE",
            "de1.exa-nodes.net",
            443,
            "6ba85179",
        )],
        relays: Vec::new(),
        routes: Vec::new(),
        // capBits = 00 00 00 03: материал per-node и запечатанные директивы.
        cap: cap::NODE_MATERIAL | cap::SEALED_DIRECTIVES,
        mirrors: Vec::new(),
        doh: Vec::new(),
        rulesets: Vec::new(),
        geo: Vec::new(),
        ttl: FIX_TTL,
        jitter: FIX_JIT,
        thresholds: Thresholds {
            conn_bytes: 8192,
            conn_packets: 22,
            resp_max: 4096,
        },
        pad_buckets: [0, 3],
        ladder: None,
        pins: Vec::new(),
        hpke: None,
    }
}

#[test]
fn minimal_catalog_payload_matches_the_worked_encoding() {
    let payload = minimal_catalog().encode().expect("модель и пределы");
    assert_eq!(
        payload.len(),
        188,
        "03-WIRE.md 8.2 фиксирует payload 188 байт"
    );
    // Карта из 12 пар, конверт с ver = 7, затем tier и открытие ex.
    let head = hex(concat!(
        "ac",
        "0101",
        "0248",
        "226e8a20f699b964",
        "0307",
        "041a",
        "6a976700",
        "051a",
        "6abef400",
        "0a01",
        "0b81",
        "ae",
    ));
    assert_eq!(
        &payload[..head.len()],
        &head[..],
        "голова payload разошлась"
    );
    // Хвост после записи узла: cap, ttl, jit, thr, pb.
    let tail = hex(concat!(
        "0e4400000003",
        "13191c20",
        "1414",
        "15a30119200002160319 1000",
        "16820003",
    )
    .replace(' ', "")
    .as_str());
    assert_eq!(
        &payload[payload.len() - tail.len()..],
        &tail[..],
        "хвост payload разошёлся"
    );
}

#[test]
fn minimal_catalog_reproduces_the_corpus_fixture_byte_for_byte() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let expected = std::fs::read(dir.join("bin/positive/c1_min.bin")).expect("c1_min.bin");

    let signed = minimal_catalog().sign(&[online()]).expect("подпись");

    assert_eq!(
        signed.frame.len(),
        expected.len(),
        "длина кадра разошлась: {} против {}",
        signed.frame.len(),
        expected.len()
    );
    if signed.frame != expected {
        let at = signed
            .frame
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap();
        panic!(
            "кадр каталога разошёлся с эталоном на смещении {at}: панель {:02x?} против корпуса {:02x?}",
            &signed.frame[at..(at + 8).min(signed.frame.len())],
            &expected[at..(at + 8).min(expected.len())]
        );
    }
    assert_eq!(
        signed.frame.len(),
        272,
        "03-WIRE.md 8.2 фиксирует 272 байта"
    );
    assert_eq!(signed.chash.to_vec(), hex(PUBLISHED_CHASH));
    assert_eq!(signed.cat_id(), PUBLISHED_CAT_ID);
}

#[test]
fn chunk_zero_of_one_reproduces_the_corpus_fixture_byte_for_byte() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let expected = std::fs::read(dir.join("bin/positive/c1c_min_0.bin")).expect("c1c_min_0.bin");

    let signed = minimal_catalog().sign(&[online()]).expect("подпись");
    // Каталог из одной части идёт тем же путём: ветки без нарезки нет.
    assert_eq!(signed.chunk_count(), 1);
    let chunk = &signed.chunks[0];

    assert_eq!(
        chunk.len(),
        expected.len(),
        "длина кадра части разошлась: {} против {}",
        chunk.len(),
        expected.len()
    );
    if *chunk != expected {
        let at = chunk
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap();
        panic!(
            "кадр части разошёлся с эталоном на смещении {at}: панель {:02x?} против корпуса {:02x?}",
            &chunk[at..(at + 8).min(chunk.len())],
            &expected[at..(at + 8).min(expected.len())]
        );
    }
    assert_eq!(chunk.len(), 407, "03-WIRE.md 8.4 фиксирует 407 байт");
    assert_eq!(csm::frame_digest(chunk).to_vec(), hex(PUBLISHED_CHUNK_SHA));

    // Та же часть через низкоуровневый путь, каким пойдёт панель при
    // повторной нарезке хранимого кадра.
    let again = catalog::chunk_frames(
        &signed.frame,
        &hex(PID).try_into().unwrap(),
        FIX_CATALOG_VER,
        FIX_IAT,
        &[online()],
    )
    .unwrap();
    assert_eq!(again, signed.chunks);
}

#[test]
fn chunk_carries_a_slice_of_the_frame_not_of_the_payload() {
    let signed = minimal_catalog().sign(&[online()]).unwrap();
    let chunks = catalog::chunk_payloads(
        &signed.frame,
        &hex(PID).try_into().unwrap(),
        FIX_CATALOG_VER,
        FIX_IAT,
    )
    .unwrap();
    assert_eq!(chunks.len(), 1);
    let c = &chunks[0];
    assert_eq!(&c.data[..5], b"CSM1\x02", "срез начинается с магии кадра");
    assert_eq!(c.data, signed.frame);
    assert_eq!(c.total_len as usize, signed.frame.len());
    assert_eq!(c.cid[..], signed.chash[..10]);
    // Конверт части: 27 байт при ver < 24, плюс 12 + 2 + 2 + 4 + 4 = 51.
    let payload = c.encode().unwrap();
    assert_eq!(payload.len() - c.data.len(), 51);
    assert_eq!(csm::DocType::CatalogChunk.as_u8(), 0x04);
    assert_eq!(DocType::Catalog.as_u8(), 0x02);
}

#[test]
fn signing_the_same_content_twice_gives_the_same_catalog() {
    // Каталог адресуется по sha256 кадра: два процесса панели обязаны
    // выпустить один тир в одни байты.
    let a = minimal_catalog().sign(&[online()]).unwrap();
    let b = minimal_catalog().sign(&[online()]).unwrap();
    assert_eq!(a, b);
}

#[test]
fn content_digest_is_stable_across_reissue_and_row_order() {
    let base = minimal_catalog();
    let mut later = base.clone();
    later.ver = 8;
    later.iat = FIX_IAT + 86_400;
    assert_eq!(
        base.content_digest(),
        later.content_digest(),
        "часы и версия не входят в дайджест содержимого"
    );

    let mut two = base.clone();
    two.exits.push(node_vless_reality(
        "n1i1",
        "\u{1F1E9}\u{1F1EA} Stealth",
        "DE",
        "de2.exa-nodes.net",
        443,
        "1f2e3d4c",
    ));
    let mut two_rev = two.clone();
    two_rev.exits.reverse();
    assert_eq!(two.content_digest(), two_rev.content_digest());
    assert_eq!(
        two.sign(&[online()]).unwrap(),
        two_rev.sign(&[online()]).unwrap(),
        "порядок строк не должен попадать в байты"
    );
    assert_ne!(base.content_digest(), two.content_digest());
}

#[test]
fn a_fleet_over_one_chunk_is_split_at_the_signed_boundary() {
    let mut c = minimal_catalog();
    c.exits = (0..40)
        .map(|i| {
            node_vless_reality(
                &format!("n{}i{}", 100 + i, 1 + i % 7),
                "\u{1F1E9}\u{1F1EA} Stealth",
                "DE",
                "de1.exa-nodes.net",
                443,
                "6ba85179",
            )
        })
        .collect();
    let signed = c.sign(&[online()]).unwrap();
    assert_eq!(
        signed.chunk_count(),
        signed.frame.len().div_ceil(CHUNK_PAYLOAD_MAX)
    );
    assert!(signed.chunk_count() >= 2);
    assert!(
        signed
            .chunks
            .iter()
            .all(|f| f.len() <= catalog::CHUNK_RESP_MAX),
        "кадр части обязан помещаться под CHUNK_RESP_MAX до набивки"
    );
}

// ---------------------------------------------------------------- цепочки релэев
//
// Корпус несёт `re` только внутри `c1_typical.bin`, где ни один из сорока
// выходов не ссылается на релей, а `cap` выставлен целиком, чтобы показать
// форму всех двенадцати битов. Форму связки «запись `re` плюс `rl` выхода,
// который в неё разрешается» там не проверяет ничего, а панель выпускает
// именно её. Эти тесты закрывают разрыв на уровне байтов и правил.

/// Узел-релей: Shadowsocks-2022, без TLS и без SNI — та самая форма, которую
/// таблица размеров `03-WIRE.md` 8.2.1 измеряет в 60 байт.
fn relay_ss2022(id: &str, cc: &str, host: &str) -> Node {
    let mut n = Node::new(
        id,
        "\u{1F1F7}\u{1F1FA} Relay",
        cc,
        host,
        8388,
        Protocol::Shadowsocks,
        Network::Tcp,
        Security::None,
    );
    n.ss_method = Some(SsMethod::Blake3Aes256Gcm);
    n
}

/// Минимальный каталог с одной цепочкой: DE-выход через RU-релей.
fn chained_catalog() -> Catalog {
    let mut c = minimal_catalog();
    c.relays = vec![relay_ss2022("n2i1", "RU", "ru-r1.exa-nodes.net")];
    c.exits[0].relay = Some("n2i1".into());
    c.cap |= cap::RELAY_CHAINING;
    c
}

#[test]
fn a_relay_entry_and_the_link_to_it_encode_as_the_field_table_says() {
    let payload = chained_catalog().encode().expect("модель и пределы");

    // `re` это ключ 12, массив из одной записи. Запись развёрнута по 8.2.1:
    // id, pn, cc, h, p, pr = 6 (shadowsocks), nw = 1 (tcp), se = 0 (none),
    // ssm = 2 (2022-blake3-aes-256-gcm). Девять пар, ни одного лишнего поля.
    let re = hex(concat!(
        "0c81",
        "a9",
        "01646e326931",
        "026ef09f87b7f09f87ba2052656c6179",
        "03625255",
        "047372752d72312e6578612d6e6f6465732e6e6574",
        "051920c4",
        "0606",
        "0701",
        "0800",
        "1702",
    ));
    assert!(
        payload.windows(re.len()).any(|w| w == re),
        "пара `re` с записью релея не найдена в payload"
    );
    // Первая пара это заголовок массива, дальше сама запись: её длина и есть
    // измеренные 60 байт формы «Shadowsocks-2022 relay» в таблице 8.2.1.
    assert_eq!(re.len() - 2, 60);

    // `rl` это ключ 22 записи выхода, tstr с `id` записи из `re`.
    let rl = hex("16646e326931");
    assert!(
        payload.windows(rl.len()).any(|w| w == rl),
        "пара `rl` выхода не найдена в payload"
    );

    // Минимальный каталог без цепочки не несёт ни того, ни другого: обе пары
    // появляются вместе и только вместе.
    let plain = minimal_catalog().encode().unwrap();
    assert!(!plain.windows(re.len()).any(|w| w == re));
    assert!(!plain.windows(rl.len()).any(|w| w == rl));

    // Кадр с цепочкой подписывается и режется тем же путём.
    let signed = chained_catalog().sign(&[online()]).expect("подпись");
    assert_eq!(signed.chunk_count(), 1);
    assert_eq!(signed.frame.len(), 272 + re.len() + rl.len());
}

#[test]
fn the_catalog_refuses_a_chain_that_does_not_resolve() {
    // Висячий `rl`: клиент такую ссылку разрешить не может, и подписывать её
    // нельзя — поэтому выпуск записи `re` и ссылка на неё это одно изменение.
    let mut dangling = minimal_catalog();
    dangling.exits[0].relay = Some("n2i1".into());
    assert!(dangling.validate().is_err(), "rl без записи в re");
    assert!(dangling.sign(&[online()]).is_err());

    // Релей не ссылается на релей: цепочка ровно в два звена.
    let mut nested = chained_catalog();
    nested.relays[0].relay = Some("n2i1".into());
    assert!(nested.validate().is_err(), "re со своим rl");

    // Один узел не может быть и выходом, и релеем: `02-SPEC.md` 4.4.
    let mut both = chained_catalog();
    both.relays.push(both.exits[0].clone());
    assert!(both.validate().is_err(), "узел в ex и re одновременно");

    // Дубликат внутри `re` ловится так же, как внутри `ex`.
    let mut dup = chained_catalog();
    dup.relays
        .push(relay_ss2022("n2i1", "FI", "fi-r1.exa-nodes.net"));
    assert!(dup.validate().is_err(), "дубликат id в re");
}

#[test]
fn the_chaining_bit_moves_the_content_digest_and_costs_one_resign() {
    // Бит 2 входит в `content_pairs`, значит поднятие его над уже
    // подписанным тиром меняет дайджест содержимого и требует ровно одной
    // переподписи: церемония обязана назвать новый `chash` тира.
    let with = chained_catalog();
    let mut without_bit = with.clone();
    without_bit.cap &= !cap::RELAY_CHAINING;
    assert_ne!(with.content_digest(), without_bit.content_digest());
    assert_ne!(
        with.sign(&[online()]).unwrap().chash,
        without_bit.sign(&[online()]).unwrap().chash
    );

    // И сам флот тоже: релей и ссылка на него это содержимое.
    assert_ne!(with.content_digest(), minimal_catalog().content_digest());

    // Порядок строк базы на дайджест по-прежнему не влияет: массив `re`
    // нормализуется по `id` как сырым байтам, наравне с `ex`.
    let mut two = chained_catalog();
    two.relays
        .push(relay_ss2022("n3i1", "FI", "fi-r1.exa-nodes.net"));
    let mut reversed = two.clone();
    reversed.relays.reverse();
    assert_eq!(two.content_digest(), reversed.content_digest());
    assert_eq!(
        two.sign(&[online()]).unwrap().frame,
        reversed.sign(&[online()]).unwrap().frame
    );
}

/// Флот с релеями и ссылками на них подписывается при опущенном бите 2 —
/// и это не лазейка, а требуемая форма выпуска (`01-DECISION.md` P8: данные
/// едут раньше контрола, иначе продаётся подделка).
///
/// Обратный перекос проверкой каталога тоже не ловится: `validate` вообще не
/// связывает `cap` с `re`, потому что бит описывает деплой оператора, а не
/// содержимое кадра. Значит честность бита держит ровно один тест, и он на
/// стороне панели, над её генератором:
/// `the_chaining_bit_follows_the_clash_generator`
/// (apps/caramba-panel/src/csm/catalog_store.rs). Здесь зафиксировано, что он
/// вправе опустить бит, не теряя ни записи `re`, ни подписи.
#[test]
fn a_fleet_may_carry_relays_with_the_chaining_bit_clear() {
    let mut data_without_control = chained_catalog();
    data_without_control.cap &= !cap::RELAY_CHAINING;
    data_without_control
        .validate()
        .expect("re и rl согласованы и без бита");
    let signed = data_without_control
        .sign(&[online()])
        .expect("каталог без бита подписывается");
    assert_eq!(signed.chunk_count(), 1);

    // Записи и ссылка на месте: клиент видит флот, но не контрол.
    assert_eq!(data_without_control.relays.len(), 1);
    assert_eq!(data_without_control.exits[0].relay.as_deref(), Some("n2i1"));

    // Перекос в другую сторону проверка тоже пропускает — здесь она не судья.
    let mut control_without_data = minimal_catalog();
    control_without_data.cap |= cap::RELAY_CHAINING;
    control_without_data
        .validate()
        .expect("cap не сверяется с re: судья не тут");
}

#[test]
fn a_selection_is_checked_against_the_catalog_that_backs_it() {
    // Два предиката `02-SPEC.md` 7.4, которые решаются только против
    // каталога. Клиент проверяет их после проверки каталога и не отвергает
    // по ним директиву; подписант держит обе стороны и обязан не выпускать
    // выбор, которого каталог не подтверждает.
    let c = chained_catalog();
    let ok = Selection {
        exit: Some("n17i3".into()),
        relay: Some("n2i1".into()),
        preset: None,
        variant: 0,
        proto: None,
        rcc: RelayResolution::Country(*b"RU"),
        nid: 17,
    };
    c.check_selection(&ok).expect("выбор подтверждён каталогом");

    let bad_exit = Selection {
        exit: Some("n99i9".into()),
        ..ok.clone()
    };
    assert!(c.check_selection(&bad_exit).is_err(), "exit вне ex");

    let bad_relay = Selection {
        relay: Some("n9i9".into()),
        ..ok.clone()
    };
    assert!(c.check_selection(&bad_relay).is_err(), "relay вне re");

    let bad_cc = Selection {
        rcc: RelayResolution::Country(*b"NL"),
        ..ok.clone()
    };
    assert!(
        c.check_selection(&bad_cc).is_err(),
        "страна записи re не равна rcc"
    );

    let contradiction = Selection {
        rcc: RelayResolution::NoRelay,
        ..ok.clone()
    };
    assert!(
        c.check_selection(&contradiction).is_err(),
        "релей назван при rcc = --"
    );

    // Выбор без релея каталогу не противоречит ничем.
    let no_relay = Selection {
        relay: None,
        rcc: RelayResolution::NoRelay,
        ..ok
    };
    c.check_selection(&no_relay).expect("без цепочки");
}

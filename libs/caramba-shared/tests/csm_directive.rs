//! Гейт директивы: панель обязана воспроизвести эталонные кадры 0x03 и 0x06
//! корпуса байт в байт из тех же входных данных, что и генератор на Go
//! (`05-TEST-VECTORS/gen/positive.go`, `docs.go`, `negative.go` buildSeals).
//!
//! Директива подписывается на каждый запрос, поэтому расхождение здесь это не
//! один испорченный документ, а каждый ответ `/sub/m1/{loc}`, отвергнутый
//! каждым клиентом. Тест не ослабляется: если байты разошлись, он называет
//! длину и первый отличающийся байт.
//!
//! Корпус лежит вне крейта, поэтому тест мягко пропускается, если его нет.

#![cfg(feature = "csm")]

use std::path::{Path, PathBuf};

use caramba_shared::csm::directive::{
    Capabilities, DeviceThumbprint, Directive, ENC_LEN, Locator, Nonce, ReasonCode,
    RelayResolution, SEAL_INFO, SealOutput, SealedDirective, Sealer, Selection, Status,
    crockford_encode, seal_aad,
};
use caramba_shared::csm::{self, DocType};
use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};

/// Момент выпуска эталонных документов, `2026-09-02T00:00:00Z`.
const FIX_IAT: u64 = 1_788_307_200;

/// Семена и производные значения из `vectors.json`, раздел `fixture_keys` и
/// `derivations`. Скопированы намеренно: тест обязан падать, если корпус
/// пересобран с другими входами, а не молча подстраиваться.
const ROOT_SEED: &str = "9aa4b46c96a0ed2aabe0391f899737224ad96032e8ca6bd53fd9daf5614d05ed";
const ONLINE_SEED: &str = "3e395bd70b7b39edf135a4610ed77446cf6b964e13daa8a9eae29402de45ff57";
const PID: &str = "226e8a20f699b964";
const LINK_PIN: &str = "49Q8M87PK6WP9QXG3T30";
const NONCE: &str = "a3f10c94b27e5d6188ff20419c73ae05";
const DTP: &str = "4f0f22569564aab09a2d1a75c132d955";
const DTP_LABEL: &[u8] = b"csm1-doc-example-device-spki";
const LOC_SECRET_LABEL: &[u8] = b"csm1-doc-example-loc-secret";
const LOC_SECRET: &str = "15bc4454d394e20a38fd6a2c29b898e48eee36ab5cf46e7d7f8286f45427d756";
const SUB_UUID: &str = "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8";
const LOC: &str = "EA3B8SKCY6VBWASE7AM1X48Y";
/// `chash` минимального каталога, из разбора полей в `03-WIRE.md` 8.3.
const CAT_MIN_HASH: &str = "eb5c33321940d11813848b8b8b03417e75fb36a82c8aa9c9567e1686f9df535d";
const DEVICE_AGREEMENT_PK: &str = "0477fcca701fd063eed7a1a41619872a76c5fc5b3adf67ce3b8edf99fcbc8a71a53655f25a0fea378a0007f5942586bb617dfb4245129a5e0cb2f83cba9382b9eb";
const SEAL_AAD: &str = "43534d3106226e8a20f699b9644f0f22569564aab09a2d1a75c132d9550000019c";

fn corpus_dir() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/caramba-client/docs/protocol/05-TEST-VECTORS");
    p.exists().then_some(p)
}

fn fixture(dir: &Path, name: &str) -> Vec<u8> {
    let p = dir.join("bin/positive").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("эталонный кадр {name}: {e}"))
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn seed(hex_seed: &str) -> SigningKey {
    let b = hex(hex_seed);
    SigningKey::from_bytes(&b.try_into().expect("32-байтовое семя"))
}

fn arr<const N: usize>(s: &str) -> [u8; N] {
    hex(s).try_into().expect("длина")
}

/// Сравнение с именованием первого расхождения: голое `assert_eq` на сотнях
/// байт бесполезно при чтении лога.
fn assert_frame(got: &[u8], want: &[u8], what: &str) {
    if got == want {
        return;
    }
    let first = got
        .iter()
        .zip(want)
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(want.len()));
    panic!(
        "{what}: кадр панели не совпал с эталонным: длина {} против {}, первое \
         расхождение на смещении {first} (панель {:?}, корпус {:?})",
        got.len(),
        want.len(),
        got.get(first),
        want.get(first)
    );
}

fn fixture_nonce() -> Nonce {
    Nonce(arr(NONCE))
}

fn fixture_dtp() -> DeviceThumbprint {
    DeviceThumbprint::of_spki(DTP_LABEL)
}

fn fixture_locator() -> Locator {
    let secret = Sha256::digest(LOC_SECRET_LABEL);
    Locator::derive(&secret, SUB_UUID, 1)
}

/// Хэш каталога берётся из самого эталонного кадра каталога: директива
/// привязана к каталогу по sha256 его полного кадра.
fn catalog_hash(dir: &Path) -> [u8; 32] {
    let frame = fixture(dir, "c1_min.bin");
    csm::frame_digest(&frame)
}

fn cap_min() -> Capabilities {
    Capabilities::from_be_bytes([0, 0, 0, 0x03]).unwrap()
}

/// Минимальная директива из `03-WIRE.md` 8.3, ровно теми же входными данными,
/// что `buildDirective` в генераторе корпуса.
fn minimal_directive(dir: &Path, ver: u64) -> Directive {
    Directive {
        pid: arr(PID),
        ver,
        iat: FIX_IAT,
        nonce: fixture_nonce(),
        dtp: fixture_dtp(),
        status: Status::Active,
        reason: ReasonCode::NONE,
        catalog: catalog_hash(dir),
        chunks: 1,
        tier: 1,
        cap: cap_min(),
        selection: None,
        policy: None,
        announce: None,
        support: None,
        hints: Vec::new(),
        ttl: 7200,
        grace: None,
        locator: fixture_locator(),
        traffic: None,
    }
}

#[test]
fn derivations_match_the_corpus() {
    assert_eq!(
        hexs(&fixture_dtp().0),
        DTP,
        "dtp считается не как в корпусе"
    );
    assert_eq!(
        hexs(&Sha256::digest(LOC_SECRET_LABEL)),
        LOC_SECRET,
        "секрет локатора корпуса"
    );
    assert_eq!(
        fixture_locator().as_str(),
        LOC,
        "локатор считается не как в корпусе"
    );
    assert_eq!(Locator::parse(LOC).unwrap(), fixture_locator());

    let root = seed(ROOT_SEED);
    let pin = crockford_encode(&Sha256::digest(root.verifying_key().to_bytes())[..12]);
    assert_eq!(pin, LINK_PIN, "link_pin считается не как в корпусе");

    let q = fixture_nonce().to_query();
    assert_eq!(q.len(), 26);
    assert_eq!(Nonce::from_query(&q).unwrap(), fixture_nonce());
    assert_eq!(
        DeviceThumbprint::from_query(&fixture_dtp().to_query()).unwrap(),
        fixture_dtp()
    );
}

#[test]
fn minimal_directive_payload_matches_the_published_walk() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    assert_eq!(hexs(&catalog_hash(&dir)), CAT_MIN_HASH, "chash каталога");

    let payload = minimal_directive(&dir, 412)
        .encode()
        .expect("поля и профиль");
    assert_eq!(
        payload.len(),
        144,
        "03-WIRE.md 8.3 фиксирует payload 144 байта"
    );

    // Разбор полей из 8.3, смещения кадра минус 7 байт заголовка.
    assert_eq!(&payload[..1], &[0xae], "карта из 14 пар");
    assert_eq!(&payload[13..17], &[0x03, 0x19, 0x01, 0x9c], "ver = 412");
    assert_eq!(&payload[65..67], &[0x0c, 0x03], "st = 3");
    assert_eq!(&payload[102..106], &[0x0f, 0x01, 0x10, 0x01], "cn, tier");
    assert_eq!(&payload[106..112], &[0x11, 0x44, 0, 0, 0, 0x03], "cap");
    assert_eq!(&payload[112..116], &[0x17, 0x19, 0x1c, 0x20], "ttl = 7200");
    assert_eq!(
        &payload[116..120],
        &[0x18, 0x19, 0x78, 0x18],
        "loc: ключ 25"
    );
    assert_eq!(&payload[120..], LOC.as_bytes());
}

#[test]
fn minimal_directive_reproduces_the_corpus_fixture_byte_for_byte() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let expected = fixture(&dir, "m1_min.bin");
    let payload = minimal_directive(&dir, 412).encode().unwrap();
    let frame = csm::build(DocType::Directive, &payload, &[seed(ONLINE_SEED)]).unwrap();

    assert_eq!(frame.len(), 228, "03-WIRE.md 8.3 фиксирует кадр 228 байт");
    assert_frame(&frame, &expected, "pos-m1-min");
    assert_eq!(
        hexs(&csm::frame_digest(&frame)),
        "b1956c4ed3877c424c1f11b903ae75be4f9a24a1537f760bb43a618da74be600",
        "опубликованный sha256 кадра"
    );
}

#[test]
fn every_status_reproduces_its_fixture() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    // Отказ едет подписанным полем, и все восемь статусов это положительные
    // векторы: клиент принимает документ, а не 403.
    let table = [
        (Status::PendingApproval, ReasonCode::AWAITING_APPROVAL),
        (Status::Onboarding, ReasonCode::NONE),
        (Status::Active, ReasonCode::NONE),
        (Status::Expired, ReasonCode::TERM_ENDED),
        (Status::Revoked, ReasonCode::DEVICE_REVOKED_BY_OPERATOR),
        (Status::Suspended, ReasonCode::ACCOUNT_SUSPENDED),
        (Status::QuotaExceeded, ReasonCode::TRAFFIC_QUOTA_EXHAUSTED),
        (Status::DeviceLimit, ReasonCode::DEVICE_LIMIT_REACHED),
    ];
    for (status, reason) in table {
        let st = status as u64;
        let mut d = minimal_directive(&dir, 420 + st);
        d.status = status;
        d.reason = reason;
        let payload = d.encode().unwrap();
        let frame = csm::build(DocType::Directive, &payload, &[seed(ONLINE_SEED)]).unwrap();
        let expected = fixture(&dir, &format!("m1_st{st}.bin"));
        assert_frame(&frame, &expected, &format!("pos-m1-st{st}"));
    }
}

#[test]
fn no_relay_sentinel_reproduces_the_fixture() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let mut d = minimal_directive(&dir, 416);
    d.selection = Some(Selection {
        exit: Some("n17i3".into()),
        relay: None,
        preset: None,
        variant: 0,
        proto: None,
        rcc: RelayResolution::NoRelay,
        nid: 17,
    });
    let payload = d.encode().unwrap();
    let frame = csm::build(DocType::Directive, &payload, &[seed(ONLINE_SEED)]).unwrap();
    assert_frame(&frame, &fixture(&dir, "m1_norelay.bin"), "pos-m1-norelay");
}

#[test]
fn padded_directives_reproduce_the_fixtures() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let online = seed(ONLINE_SEED);

    let frame = minimal_directive(&dir, 414)
        .sign(std::slice::from_ref(&online), 0)
        .unwrap();
    assert_eq!(frame.len(), 256);
    assert_frame(
        &frame,
        &fixture(&dir, "m1_padded_r0.bin"),
        "pos-m1-padded-r0",
    );

    let frame = minimal_directive(&dir, 415).sign(&[online], 3).unwrap();
    assert_eq!(frame.len(), 1024);
    assert_frame(
        &frame,
        &fixture(&dir, "m1_padded_r3.bin"),
        "pos-m1-padded-r3",
    );
}

#[test]
fn signing_is_deterministic_per_request() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let online = seed(ONLINE_SEED);
    let d = minimal_directive(&dir, 412);
    assert_eq!(
        d.sign(std::slice::from_ref(&online), 1).unwrap(),
        d.sign(&[online], 1).unwrap()
    );
}

/// Заглушка HPKE: отдаёт `enc` и `ct` из эталонного запечатанного кадра, но
/// сверяет всё, что панель обязана подать на вход настоящей реализации.
struct FixtureSealer {
    enc: [u8; ENC_LEN],
    ct: Vec<u8>,
    expect_plaintext: Vec<u8>,
}

impl Sealer for FixtureSealer {
    fn seal(
        &self,
        recipient_pk: &[u8; ENC_LEN],
        info: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<SealOutput, caramba_shared::csm::directive::DirectiveError> {
        assert_eq!(hexs(recipient_pk), DEVICE_AGREEMENT_PK, "ключ получателя");
        assert_eq!(info, SEAL_INFO);
        assert_eq!(hexs(aad), SEAL_AAD, "aad из vectors.json hpke.aad_fixture");
        assert_eq!(
            plaintext, self.expect_plaintext,
            "открытый текст это кадр 0x03"
        );
        Ok(SealOutput {
            enc: self.enc,
            ct: self.ct.clone(),
        })
    }
}

/// Смещения `enc` и `ct` в `m1s_min.bin` из таблицы `03-WIRE.md` 9.6:
/// 7 байт заголовка, 29 конверта, 18 `dtp`, 6 идентификаторов набора,
/// голова `0e 58 41`, затем 65 байт `enc`; голова `0f 58 f4`, затем 244 байта.
const ENC_AT: usize = 7 + 29 + 18 + 6 + 3;
const CT_AT: usize = ENC_AT + ENC_LEN + 3;
const CT_LEN: usize = 228 + 16;

#[test]
fn sealed_wrapper_reproduces_the_fixture_framing() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let online = seed(ONLINE_SEED);
    let inner_expected = fixture(&dir, "m1s_min.bin");
    let enc: [u8; ENC_LEN] = inner_expected[ENC_AT..ENC_AT + ENC_LEN].try_into().unwrap();
    let ct = inner_expected[CT_AT..CT_AT + CT_LEN].to_vec();
    assert_eq!(enc[0], 0x04, "enc это несжатая точка");

    let d = minimal_directive(&dir, 412);
    assert_eq!(hexs(&seal_aad(&d.pid, &d.dtp, d.ver as u32)), SEAL_AAD);

    let inner = csm::build(
        DocType::Directive,
        &d.encode().unwrap(),
        std::slice::from_ref(&online),
    )
    .unwrap();
    assert_frame(&inner, &fixture(&dir, "m1_min.bin"), "внутренний кадр");

    let sealer = FixtureSealer {
        enc,
        ct,
        expect_plaintext: inner.clone(),
    };
    let sealed = SealedDirective::wrap(&d, &inner, &arr(DEVICE_AGREEMENT_PK), 1, &sealer).unwrap();
    let payload = sealed.encode().unwrap();
    assert_eq!(
        payload.len(),
        370,
        "03-WIRE.md 9.6: внешний payload 370 байт"
    );
    let frame = csm::build(
        DocType::SealedDirective,
        &payload,
        std::slice::from_ref(&online),
    )
    .unwrap();
    assert_eq!(frame.len(), 454, "cor-2: запечатанный кадр 454 байта");
    assert_frame(&frame, &inner_expected, "pos-m1s-min");

    let padded = sealed.sign(&[online], 0).unwrap();
    assert_eq!(padded.len(), 512);
    assert_frame(&padded, &fixture(&dir, "m1s_padded.bin"), "pos-m1s-padded");
}

#[test]
fn full_panel_path_lands_on_the_grid() {
    let Some(dir) = corpus_dir() else {
        eprintln!("корпус недоступен, тест пропущен");
        return;
    };
    let online = seed(ONLINE_SEED);
    let d = minimal_directive(&dir, 412);

    // Внутренний кадр с r = 0 ложится на 256; заглушка возвращает шифртекст
    // той же длины плюс тег, как сделала бы настоящая реализация.
    let inner = d.sign(std::slice::from_ref(&online), 0).unwrap();
    assert_eq!(inner.len(), 256);
    let mut ct = inner.clone();
    ct.extend_from_slice(&[0u8; 16]);
    let mut enc = [0u8; ENC_LEN];
    enc[0] = 0x04;
    struct GridSealer(SealOutput);
    impl Sealer for GridSealer {
        fn seal(
            &self,
            _: &[u8; ENC_LEN],
            _: &[u8],
            _: &[u8],
            _: &[u8],
        ) -> Result<SealOutput, caramba_shared::csm::directive::DirectiveError> {
            Ok(self.0.clone())
        }
    }
    let sealer = GridSealer(SealOutput { enc, ct });
    let frame = d
        .sign_sealed(&[online], 0, &arr(DEVICE_AGREEMENT_PK), 1, &sealer, 0)
        .unwrap();
    assert_eq!(&frame[..5], b"CSM1\x06");
    assert_eq!(frame.len(), 512, "ответ на сетке 256 байт");
}

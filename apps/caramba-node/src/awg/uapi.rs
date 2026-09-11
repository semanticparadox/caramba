//! Управление интерфейсом через UAPI-сокет amneziawg-go.
//!
//! Почему не утилита `awg` из amneziawg-tools: её пришлось бы ставить на все
//! узлы отдельным пакетом (на veles нет даже iptables), а протокол UAPI —
//! обычный текст поверх unix-сокета, который amneziawg-go открывает сам.
//! Так у узла нет ни одной новой внешней зависимости.
//!
//! Формат протокола (наследство WireGuard):
//!   запрос  `set=1\n<key>=<value>\n…\n\n`  или  `get=1\n\n`
//!   ответ   `<key>=<value>\n…\nerrno=0\n\n`
//! Ключи в UAPI — ВСЕГДА hex, а панель хранит их в base64: конвертация здесь.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::AwgSection;

/// Каталоги, где amneziawg-go может открыть UAPI-сокет.
///
/// Апстрим — форк wireguard-go, и путь сокета там задаётся переменной, которую
/// правят линкером при сборке; у разных сборок он отличается. Поэтому не
/// угадываем, а перебираем: лишняя проверка `Path::exists` стоит наносекунды,
/// а ошибка в угаданном пути стоила бы неработающего AWG на узле.
pub const SOCKET_DIRS: [&str; 4] = [
    "/var/run/amneziawg",
    "/run/amneziawg",
    "/var/run/wireguard",
    "/run/wireguard",
];

/// Возможные пути сокета для интерфейса — в порядке предпочтения.
pub fn socket_candidates(iface: &str) -> Vec<PathBuf> {
    SOCKET_DIRS
        .iter()
        .map(|dir| PathBuf::from(format!("{dir}/{iface}.sock")))
        .collect()
}

/// Первый существующий сокет интерфейса.
pub fn find_socket(iface: &str) -> Option<PathBuf> {
    socket_candidates(iface).into_iter().find(|p| p.exists())
}

/// Ждать появления сокета после запуска процесса.
///
/// amneziawg-go демонизируется и открывает сокет не мгновенно; без ожидания
/// первая же конфигурация ушла бы в никуда и узел применил бы её только через
/// две минуты, на следующей проверке конфига.
pub async fn wait_for_socket(iface: &str, limit: Duration) -> Option<PathBuf> {
    let deadline = std::time::Instant::now() + limit;
    loop {
        if let Some(path) = find_socket(iface) {
            return Some(path);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Ключ WireGuard в base64 (любой из ходовых алфавитов) → hex для UAPI.
///
/// Панель отдаёт ключи так, как их напечатал `sing-box generate
/// wireguard-keypair` (обычный base64 с паддингом), но reality-ключи в той же
/// кодовой базе лежат в URL-safe без паддинга. Принимаем оба алфавита: цена —
/// три лишние попытки декодирования, выигрыш — узел не ломается от того, каким
/// генератором панель воспользовалась.
pub fn key_base64_to_hex(value: &str) -> anyhow::Result<String> {
    use base64::Engine;
    use base64::engine::GeneralPurpose;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("пустой ключ");
    }
    // Конкретный тип, а не `dyn Engine`: у трейта дженерик-методы, и объектом
    // он не бывает. Все четыре алфавита всё равно одного типа.
    let engines: [&GeneralPurpose; 4] = [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ];
    for engine in engines {
        if let Ok(bytes) = engine.decode(trimmed) {
            if bytes.len() != 32 {
                anyhow::bail!("ключ длиной {} байт вместо 32", bytes.len());
            }
            return Ok(hex::encode(bytes));
        }
    }
    anyhow::bail!("ключ не разобран как base64")
}

/// Состояние одного пира, как его отдаёт `get=1`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerState {
    /// Публичный ключ в hex — ровно так, как его вернул UAPI.
    pub public_key_hex: String,
    /// Байты, ПРИНЯТЫЕ узлом от пира. Это выгрузка пользователя.
    pub rx_bytes: u64,
    /// Байты, ОТПРАВЛЕННЫЕ узлом пиру. Это загрузка пользователя.
    pub tx_bytes: u64,
    /// Unix-время последнего хендшейка, 0 — хендшейка не было ни разу.
    pub last_handshake_sec: u64,
    pub allowed_ips: Vec<String>,
}

/// Состояние устройства целиком.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    pub private_key_hex: String,
    pub listen_port: u16,
    /// Параметры обфускации, которые устройство сообщает о себе. Нулевые
    /// значения UAPI не отдаёт вовсе — отсюда ноль по умолчанию.
    pub jc: u16,
    pub jmin: u16,
    pub jmax: u16,
    pub s1: u16,
    pub s2: u16,
    pub h1: u32,
    pub h2: u32,
    pub h3: u32,
    pub h4: u32,
    pub peers: Vec<PeerState>,
}

impl DeviceState {
    fn peer_index(&self) -> HashMap<&str, &PeerState> {
        self.peers
            .iter()
            .map(|p| (p.public_key_hex.as_str(), p))
            .collect()
    }
}

/// Разбор ответа `get=1`.
///
/// Всё, что до первого `public_key=`, относится к устройству; дальше идут пиры,
/// каждый начинается со своего `public_key=`. Неизвестные ключи пропускаются:
/// у AWG 2.x/3.x их заметно больше, и падать на них незачем.
pub fn parse_get_response(body: &str) -> anyhow::Result<DeviceState> {
    let mut device = DeviceState::default();
    let mut peers: Vec<PeerState> = Vec::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key == "errno" {
            let code: i64 = value.trim().parse().unwrap_or(-1);
            if code != 0 {
                anyhow::bail!("UAPI вернул errno={code}");
            }
            continue;
        }
        if key == "public_key" {
            peers.push(PeerState {
                public_key_hex: value.trim().to_ascii_lowercase(),
                ..Default::default()
            });
            continue;
        }

        if let Some(peer) = peers.last_mut() {
            match key {
                "rx_bytes" => peer.rx_bytes = value.trim().parse().unwrap_or(0),
                "tx_bytes" => peer.tx_bytes = value.trim().parse().unwrap_or(0),
                "last_handshake_time_sec" => {
                    peer.last_handshake_sec = value.trim().parse().unwrap_or(0)
                }
                "allowed_ip" => peer.allowed_ips.push(value.trim().to_string()),
                _ => {}
            }
            continue;
        }

        match key {
            "private_key" => device.private_key_hex = value.trim().to_ascii_lowercase(),
            "listen_port" => device.listen_port = value.trim().parse().unwrap_or(0),
            "jc" => device.jc = value.trim().parse().unwrap_or(0),
            "jmin" => device.jmin = value.trim().parse().unwrap_or(0),
            "jmax" => device.jmax = value.trim().parse().unwrap_or(0),
            "s1" => device.s1 = value.trim().parse().unwrap_or(0),
            "s2" => device.s2 = value.trim().parse().unwrap_or(0),
            "h1" => device.h1 = value.trim().parse().unwrap_or(0),
            "h2" => device.h2 = value.trim().parse().unwrap_or(0),
            "h3" => device.h3 = value.trim().parse().unwrap_or(0),
            "h4" => device.h4 = value.trim().parse().unwrap_or(0),
            _ => {}
        }
    }

    device.peers = peers;
    Ok(device)
}

/// Собрать `set=1` из разницы между текущим и желаемым состоянием.
///
/// Именно разницу, а не полную перезапись: `replace_peers=true` снёс бы все
/// пиры и оборвал живые сессии у всех разом при добавлении одного нового
/// пользователя. Пир, который не изменился, в запрос не попадает вовсе —
/// его хендшейк и счётчики остаются нетронутыми.
///
/// Когда менять нечего, возвращается одна строка `set=1` — вызывающий код по
/// числу строк понимает, что идти в сокет не нужно.
pub fn build_set_request(
    current: Option<&DeviceState>,
    desired: &AwgSection,
) -> anyhow::Result<String> {
    let mut out = String::from("set=1\n");

    let private_hex = key_base64_to_hex(&desired.private_key)?;
    let empty = DeviceState::default();
    let cur = current.unwrap_or(&empty);
    let fresh = current.is_none();

    if fresh || cur.private_key_hex != private_hex {
        out.push_str(&format!("private_key={private_hex}\n"));
    }
    if fresh || cur.listen_port != desired.listen_port {
        out.push_str(&format!("listen_port={}\n", desired.listen_port));
    }
    // Параметры обфускации перечислены поимённо, а не циклом: каждый из них
    // должен попасть в запрос ровно под своим именем, и опечатка здесь тише
    // всего проявилась бы как «AWG работает, но не обходит DPI».
    for (name, cur_v, want_v) in [
        ("jc", cur.jc as u32, desired.jc as u32),
        ("jmin", cur.jmin as u32, desired.jmin as u32),
        ("jmax", cur.jmax as u32, desired.jmax as u32),
        ("s1", cur.s1 as u32, desired.s1 as u32),
        ("s2", cur.s2 as u32, desired.s2 as u32),
        ("h1", cur.h1, desired.h1),
        ("h2", cur.h2, desired.h2),
        ("h3", cur.h3, desired.h3),
        ("h4", cur.h4, desired.h4),
    ] {
        if fresh || cur_v != want_v {
            out.push_str(&format!("{name}={want_v}\n"));
        }
    }

    let index = cur.peer_index();
    let mut wanted_hex: Vec<String> = Vec::with_capacity(desired.peers.len());

    for peer in &desired.peers {
        let hex_key = key_base64_to_hex(&peer.public_key)?;
        wanted_hex.push(hex_key.clone());
        let allowed = peer.allowed_ip.trim();
        match index.get(hex_key.as_str()) {
            // Пир уже такой, какой нужен — молчим, чтобы не тронуть сессию.
            Some(existing) if existing.allowed_ips == vec![allowed.to_string()] => {}
            _ => {
                out.push_str(&format!("public_key={hex_key}\n"));
                out.push_str("replace_allowed_ips=true\n");
                out.push_str(&format!("allowed_ip={allowed}\n"));
            }
        }
    }

    for existing in &cur.peers {
        if !wanted_hex.iter().any(|k| k == &existing.public_key_hex) {
            out.push_str(&format!("public_key={}\n", existing.public_key_hex));
            out.push_str("remove=true\n");
        }
    }

    Ok(out)
}

/// Один обмен с UAPI: пишем запрос, читаем ответ до пустой строки.
///
/// amneziawg-go НЕ закрывает соединение после ответа (проверено на Canada,
/// v3.1.20260828: `get=1` отвечает и держит сокет открытым). Чтение «до EOF»
/// здесь упиралось в таймаут на каждом обмене, хотя `set=1` уже применился —
/// узел считал конфигурацию непринятой, не поднимал адрес и маршруты и бесконечно
/// повторял попытки. Конец ответа по протоколу — пустая строка после `errno=N`,
/// её и ждём. Таймаут остаётся: зависший сокет не должен останавливать цикл
/// агента, на котором держится вся управляемость узла.
async fn roundtrip(socket: &Path, request: &str) -> anyhow::Result<String> {
    let io = async {
        let mut stream = tokio::net::UnixStream::connect(socket).await?;
        stream.write_all(request.as_bytes()).await?;
        // Пустая строка — конец запроса в UAPI.
        stream.write_all(b"\n").await?;
        stream.flush().await?;
        let mut raw = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
            if response_complete(&raw) {
                break;
            }
        }
        Ok::<String, std::io::Error>(String::from_utf8_lossy(&raw).into_owned())
    };

    match tokio::time::timeout(Duration::from_secs(5), io).await {
        Ok(Ok(body)) => Ok(body),
        Ok(Err(e)) => Err(anyhow::anyhow!("UAPI {}: {e}", socket.display())),
        Err(_) => Err(anyhow::anyhow!(
            "UAPI {} не ответил за 5 с",
            socket.display()
        )),
    }
}

/// Ответ UAPI закончен, когда пришла пустая строка-терминатор.
///
/// Пустой ответ (`\n\n` без единого ключа) тоже считается законченным:
/// amneziawg-go так отвечает на `set=1`, если ему нечего сообщить кроме errno,
/// а errno у него всегда есть — но полагаться на это не будем.
fn response_complete(raw: &[u8]) -> bool {
    raw.ends_with(b"\n\n")
}

/// Текущее состояние устройства.
pub async fn get_device(socket: &Path) -> anyhow::Result<DeviceState> {
    let body = roundtrip(socket, "get=1\n").await?;
    parse_get_response(&body)
}

/// Применить подготовленный `set=1`.
pub async fn set_device(socket: &Path, request: &str) -> anyhow::Result<()> {
    let body = roundtrip(socket, request).await?;
    for line in body.lines() {
        if let Some(code) = line.trim().strip_prefix("errno=") {
            let code: i64 = code.trim().parse().unwrap_or(-1);
            if code == 0 {
                return Ok(());
            }
            anyhow::bail!("UAPI отверг конфигурацию: errno={code}");
        }
    }
    anyhow::bail!("UAPI не вернул errno — конфигурация не подтверждена")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::awg::{AwgPeer, test_section};

    const PRIV_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const PRIV_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const PEER_B64: &str = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWY=";
    const PEER_HEX: &str = "4142434445464748494a4b4c4d4e4f505152535455565758595a616263646566";

    /// Тот же раздел, что и в тестах контракта: запрос к UAPI обязан
    /// собираться именно из него, а не из подогнанной под тест структуры.
    fn section() -> AwgSection {
        test_section()
    }

    /// UAPI понимает только hex. Панель хранит base64. Ошибка в этом переводе
    /// означала бы, что сервер ждёт пиров с чужими ключами.
    #[test]
    fn keys_are_translated_from_base64_to_hex() {
        assert_eq!(key_base64_to_hex(PRIV_B64).unwrap(), PRIV_HEX);
        assert_eq!(key_base64_to_hex(PEER_B64).unwrap(), PEER_HEX);
    }

    /// В кодовой базе соседствуют два алфавита base64 (sing-box печатает
    /// обычный, reality-ключи лежат в URL-safe без паддинга) — принимаем оба.
    #[test]
    fn both_base64_alphabets_are_accepted() {
        let url_safe = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWY";
        assert_eq!(key_base64_to_hex(url_safe).unwrap(), PEER_HEX);
    }

    /// Ключ не из 32 байт — чужой формат; тихо подставлять его в UAPI нельзя.
    #[test]
    fn a_key_of_the_wrong_size_is_an_error() {
        assert!(key_base64_to_hex("c2hvcnQ=").is_err());
        assert!(key_base64_to_hex("").is_err());
        assert!(key_base64_to_hex("не base64 вовсе").is_err());
    }

    const GET_SAMPLE: &str = "private_key=0000000000000000000000000000000000000000000000000000000000000000\n\
listen_port=51820\n\
jc=4\n\
jmin=40\n\
jmax=70\n\
s1=30\n\
s2=40\n\
h1=1148476\n\
h2=2148476\n\
h3=3148476\n\
h4=4148476\n\
public_key=4142434445464748494a4b4c4d4e4f505152535455565758595a616263646566\n\
protocol_version=1\n\
endpoint=203.0.113.9:41234\n\
last_handshake_time_sec=1757600000\n\
last_handshake_time_nsec=123\n\
tx_bytes=2048\n\
rx_bytes=1024\n\
persistent_keepalive_interval=0\n\
allowed_ip=10.66.0.7/32\n\
errno=0\n\n";

    /// Разбор ответа устройства: и параметры обфускации, и счётчики пира.
    #[test]
    fn a_get_response_is_parsed_into_device_and_peers() {
        let state = parse_get_response(GET_SAMPLE).unwrap();
        assert_eq!(state.private_key_hex, PRIV_HEX);
        assert_eq!(state.listen_port, 51820);
        assert_eq!((state.jc, state.jmin, state.jmax), (4, 40, 70));
        assert_eq!(state.h4, 4_148_476);
        assert_eq!(state.peers.len(), 1);
        let peer = &state.peers[0];
        assert_eq!(peer.public_key_hex, PEER_HEX);
        assert_eq!(peer.rx_bytes, 1024);
        assert_eq!(peer.tx_bytes, 2048);
        assert_eq!(peer.last_handshake_sec, 1_757_600_000);
        assert_eq!(peer.allowed_ips, vec!["10.66.0.7/32".to_string()]);
    }

    /// Счётчики пира не должны прилипать к устройству: `rx_bytes` идёт после
    /// `public_key`, и спутать эти две области — значит потерять весь трафик.
    #[test]
    fn device_fields_after_the_first_peer_belong_to_the_peer() {
        let state = parse_get_response(GET_SAMPLE).unwrap();
        assert_eq!(state.peers[0].rx_bytes, 1024);
        assert_eq!(state.listen_port, 51820);
    }

    /// Ненулевой errno — отказ, а не данные.
    #[test]
    fn a_non_zero_errno_is_an_error() {
        assert!(parse_get_response("errno=-22\n\n").is_err());
    }

    /// Незнакомые ключи (AWG 2.x/3.x добавляет i1..i5, s3, s4) не ломают разбор.
    #[test]
    fn unknown_uapi_keys_are_skipped() {
        let body = format!("i1=b0xxxx\nitime=30\n{GET_SAMPLE}");
        assert!(parse_get_response(&body).is_ok());
    }

    /// Чистое устройство: в запрос попадает всё, включая ключ и обфускацию.
    #[test]
    fn a_fresh_device_is_configured_from_scratch() {
        let req = build_set_request(None, &section()).unwrap();
        assert!(req.starts_with("set=1\n"));
        assert!(req.contains(&format!("private_key={PRIV_HEX}\n")));
        assert!(req.contains("listen_port=51820\n"));
        assert!(req.contains("jc=4\n"));
        assert!(req.contains("h1=1148476\n"));
        assert!(req.contains(&format!("public_key={PEER_HEX}\n")));
        assert!(req.contains("allowed_ip=10.66.0.7/32\n"));
    }

    /// Ничего не изменилось — запрос пустой. Это и есть «синк без обрыва»:
    /// в сокет мы даже не пойдём.
    #[test]
    fn an_unchanged_device_produces_an_empty_request() {
        let current = parse_get_response(GET_SAMPLE).unwrap();
        let req = build_set_request(Some(&current), &section()).unwrap();
        assert_eq!(req, "set=1\n");
    }

    /// Добавили пользователя — в запросе только он. Существующий пир не
    /// упоминается, значит его хендшейк и счётчики уцелеют.
    #[test]
    fn adding_a_peer_does_not_touch_the_existing_one() {
        let current = parse_get_response(GET_SAMPLE).unwrap();
        let mut desired = section();
        desired.peers.push(AwgPeer {
            public_key: "YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=".to_string(),
            allowed_ip: "10.66.0.8/32".to_string(),
            user_tag: "user_43".to_string(),
        });
        let req = build_set_request(Some(&current), &desired).unwrap();
        assert!(
            !req.contains(PEER_HEX),
            "старый пир не должен переписываться"
        );
        assert!(req.contains("allowed_ip=10.66.0.8/32\n"));
        assert!(!req.contains("replace_peers"), "полная замена рвёт сессии");
    }

    /// Пользователь исчез из панели — пир снимается точечно.
    #[test]
    fn a_peer_missing_from_the_panel_is_removed() {
        let current = parse_get_response(GET_SAMPLE).unwrap();
        let mut desired = section();
        desired.peers.clear();
        let req = build_set_request(Some(&current), &desired).unwrap();
        assert!(req.contains(&format!("public_key={PEER_HEX}\nremove=true\n")));
    }

    /// Сменился адрес пира — переписываем только allowed_ips этого пира.
    #[test]
    fn a_changed_allowed_ip_rewrites_only_that_peer() {
        let current = parse_get_response(GET_SAMPLE).unwrap();
        let mut desired = section();
        desired.peers[0].allowed_ip = "10.66.1.9/32".to_string();
        let req = build_set_request(Some(&current), &desired).unwrap();
        assert!(req.contains("replace_allowed_ips=true\n"));
        assert!(req.contains("allowed_ip=10.66.1.9/32\n"));
        assert!(!req.contains("remove=true"));
    }

    /// Панель повернула ключ сервера — он обязан уехать в устройство, иначе
    /// клиенты с новым конфигом не подключатся вообще.
    #[test]
    fn a_rotated_server_key_is_pushed() {
        let current = parse_get_response(GET_SAMPLE).unwrap();
        let mut desired = section();
        desired.private_key = PEER_B64.to_string();
        let req = build_set_request(Some(&current), &desired).unwrap();
        assert!(req.contains(&format!("private_key={PEER_HEX}\n")));
    }

    /// Короткий путь: у unix-сокета лимит ~104 байта, а $TMPDIR на macOS длинный.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cawg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Сервер, который отвечает и НЕ закрывает соединение — ровно так ведёт
    /// себя amneziawg-go. До правки roundtrip ждал EOF и всегда упирался в
    /// таймаут; теперь обмен обязан закончиться по пустой строке за доли секунды.
    async fn serve_without_closing(
        dir: &Path,
        reply: &'static str,
    ) -> (PathBuf, tokio::task::JoinHandle<Vec<u8>>) {
        let socket = dir.join("awg-test.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let n = stream.read(&mut chunk).await.unwrap();
                seen.extend_from_slice(&chunk[..n]);
                if n == 0 || seen.ends_with(b"\n\n") {
                    break;
                }
            }
            stream.write_all(reply.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            // Держим соединение открытым дольше таймаута клиента: если клиент
            // всё ещё ждёт EOF, тест упадёт по таймауту, а не пройдёт случайно.
            tokio::time::sleep(Duration::from_secs(7)).await;
            seen
        });
        (socket, handle)
    }

    #[tokio::test]
    async fn get_completes_without_eof_from_the_server() {
        let dir = scratch_dir("get");
        let (socket, server) =
            serve_without_closing(&dir, "private_key=00\nlisten_port=17400\nerrno=0\n\n").await;
        let started = std::time::Instant::now();
        let body = roundtrip(&socket, "get=1\n").await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "ответ ждали до EOF"
        );
        assert!(body.ends_with("errno=0\n\n"));
        server.abort();
    }

    #[tokio::test]
    async fn set_is_confirmed_by_errno_without_eof() {
        let dir = scratch_dir("set");
        let (socket, server) = serve_without_closing(&dir, "errno=0\n\n").await;
        set_device(&socket, "set=1\nlisten_port=17400\n")
            .await
            .unwrap();
        server.abort();
    }

    #[test]
    fn a_response_is_complete_only_at_the_blank_line() {
        assert!(!response_complete(b"errno=0\n"));
        assert!(!response_complete(b"private_key=00\n"));
        assert!(response_complete(b"errno=0\n\n"));
        assert!(response_complete(b"\n\n"));
    }

    /// Сокет ищем перебором каталогов — путь у разных сборок разный.
    #[test]
    fn socket_candidates_cover_both_naming_schemes() {
        let paths: Vec<String> = socket_candidates("awg0")
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert!(paths.contains(&"/var/run/amneziawg/awg0.sock".to_string()));
        assert!(paths.contains(&"/var/run/wireguard/awg0.sock".to_string()));
    }
}

//! Учёт трафика и онлайна AWG-пиров.
//!
//! Счётчики в UAPI накопительные, как и у sing-box, поэтому дельты считаем у
//! себя и по той же логике: потерянный опрос лишь откладывает учёт, а обнулять
//! счётчики на стороне ядра нельзя — один пропущенный ответ означал бы
//! безвозвратно потерянный трафик.
//!
//! «Онлайн» здесь принципиально другой, чем у sing-box: у WireGuard нет
//! соединений, которые можно посчитать, зато есть хендшейк раз в ~120 с.
//! Отсюда окно в 180 с — минимальное, при котором живой пир не мигает.

use std::collections::HashMap;

use super::uapi::PeerState;

/// Что узел сообщает панели про одного пользователя за интервал.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerUsage {
    pub tag: String,
    /// Байты, принятые узлом от пользователя (его выгрузка).
    pub rx_delta: u64,
    /// Байты, отданные узлом пользователю (его загрузка).
    pub tx_delta: u64,
    pub online: bool,
}

/// Текущее время в секундах Unix.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Свежесть хендшейка → онлайн.
///
/// Нулевой хендшейк — пир заведён, но ни разу не подключался: это не онлайн.
/// Хендшейк из будущего (часы узла уехали) считаем свежим: иначе перевод
/// времени выключил бы всех разом.
pub fn is_online(last_handshake_sec: u64, now: u64, window: u64) -> bool {
    if last_handshake_sec == 0 {
        return false;
    }
    now.saturating_sub(last_handshake_sec) < window
}

/// Дельты по каждому пиру плюс признак онлайна.
///
/// `last` — накопительные счётчики с прошлого опроса, ключ — публичный ключ в
/// hex (именно его отдаёт UAPI). Пиры, которых панель больше не присылает,
/// выпадают из карты, чтобы она не росла вечно.
///
/// В результат попадают только те, у кого есть что сказать: либо прошёл
/// трафик, либо пир онлайн. Молчащие офлайн-пиры раздували бы heartbeat на
/// сотни записей ни о чём.
pub fn fold_deltas(
    last: &mut HashMap<String, (u64, u64)>,
    peers: &[PeerState],
    tags_by_hex: &HashMap<String, String>,
    now: u64,
    window: u64,
) -> Vec<PeerUsage> {
    let mut out = Vec::new();
    let mut seen = Vec::with_capacity(peers.len());

    for peer in peers {
        let key = peer.public_key_hex.to_ascii_lowercase();
        seen.push(key.clone());

        let Some(tag) = tags_by_hex.get(&key) else {
            // Пир есть на интерфейсе, но панель про него не знает: это остаток
            // от прошлой конфигурации. Трафик такого пира приписать некому.
            continue;
        };

        let (prev_rx, prev_tx) = last.get(&key).copied().unwrap_or((0, 0));
        // Счётчик уехал вниз — amneziawg-go перезапустили. Отдаём наблюдаемое
        // значение один раз, иначе трафик после рестарта потерялся бы.
        let rx_delta = if peer.rx_bytes >= prev_rx {
            peer.rx_bytes - prev_rx
        } else {
            peer.rx_bytes
        };
        let tx_delta = if peer.tx_bytes >= prev_tx {
            peer.tx_bytes - prev_tx
        } else {
            peer.tx_bytes
        };
        last.insert(key, (peer.rx_bytes, peer.tx_bytes));

        let online = is_online(peer.last_handshake_sec, now, window);
        if rx_delta > 0 || tx_delta > 0 || online {
            out.push(PeerUsage {
                tag: tag.clone(),
                rx_delta,
                tx_delta,
                online,
            });
        }
    }

    last.retain(|key, _| seen.iter().any(|k| k == key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(hex: &str, rx: u64, tx: u64, handshake: u64) -> PeerState {
        PeerState {
            public_key_hex: hex.to_string(),
            rx_bytes: rx,
            tx_bytes: tx,
            last_handshake_sec: handshake,
            allowed_ips: vec!["10.66.0.7/32".to_string()],
        }
    }

    fn tags() -> HashMap<String, String> {
        HashMap::from([("aa".to_string(), "user_42".to_string())])
    }

    /// Первый замер: дельта равна всему, что успело накопиться. Иначе трафик
    /// до первого опроса просто пропал бы.
    #[test]
    fn the_first_reading_counts_everything_seen() {
        let mut last = HashMap::new();
        let out = fold_deltas(
            &mut last,
            &[peer("aa", 100, 200, 1_000)],
            &tags(),
            1_050,
            180,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rx_delta, 100);
        assert_eq!(out[0].tx_delta, 200);
        assert!(out[0].online);
    }

    /// Второй замер отдаёт только прирост, а не всю сумму заново — иначе
    /// квоты сгорали бы за считанные часы.
    #[test]
    fn the_second_reading_reports_only_the_increment() {
        let mut last = HashMap::new();
        fold_deltas(
            &mut last,
            &[peer("aa", 100, 200, 1_000)],
            &tags(),
            1_050,
            180,
        );
        let out = fold_deltas(
            &mut last,
            &[peer("aa", 150, 260, 1_100)],
            &tags(),
            1_150,
            180,
        );
        assert_eq!(out[0].rx_delta, 50);
        assert_eq!(out[0].tx_delta, 60);
    }

    /// amneziawg-go перезапустили, счётчики обнулились: отдаём наблюдаемое
    /// значение один раз, а не отрицательную дельту.
    #[test]
    fn a_counter_reset_is_reported_once_and_not_lost() {
        let mut last = HashMap::new();
        fold_deltas(
            &mut last,
            &[peer("aa", 5_000, 9_000, 1_000)],
            &tags(),
            1_050,
            180,
        );
        let out = fold_deltas(&mut last, &[peer("aa", 40, 70, 1_100)], &tags(), 1_150, 180);
        assert_eq!(out[0].rx_delta, 40);
        assert_eq!(out[0].tx_delta, 70);
    }

    /// Онлайн считается по свежести хендшейка, а не по трафику: пир может
    /// молчать и при этом быть подключённым.
    #[test]
    fn a_silent_but_fresh_peer_is_still_online() {
        let mut last = HashMap::new();
        fold_deltas(
            &mut last,
            &[peer("aa", 100, 200, 1_000)],
            &tags(),
            1_050,
            180,
        );
        let out = fold_deltas(
            &mut last,
            &[peer("aa", 100, 200, 1_100)],
            &tags(),
            1_150,
            180,
        );
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].rx_delta, out[0].tx_delta), (0, 0));
        assert!(out[0].online);
    }

    /// Старый хендшейк — офлайн, и без трафика такой пир вообще не попадает
    /// в heartbeat.
    #[test]
    fn a_stale_peer_without_traffic_is_not_reported() {
        let mut last = HashMap::new();
        fold_deltas(
            &mut last,
            &[peer("aa", 100, 200, 1_000)],
            &tags(),
            1_050,
            180,
        );
        let out = fold_deltas(
            &mut last,
            &[peer("aa", 100, 200, 1_000)],
            &tags(),
            5_000,
            180,
        );
        assert!(out.is_empty());
    }

    /// Граница окна: ровно 180 с — уже не онлайн, 179 — ещё онлайн.
    #[test]
    fn the_online_window_boundary_is_exact() {
        assert!(is_online(1_000, 1_179, 180));
        assert!(!is_online(1_000, 1_180, 180));
        assert!(!is_online(0, 1_000, 180), "пир без хендшейка не онлайн");
    }

    /// Часы узла уехали вперёд — пир не должен разом «пропасть».
    #[test]
    fn a_handshake_from_the_future_does_not_drop_the_peer() {
        assert!(is_online(2_000, 1_000, 180));
    }

    /// Пир, которого нет в списке панели, учесть некому — он пропускается,
    /// а не приписывается случайному тегу.
    #[test]
    fn an_unknown_peer_is_not_attributed_to_anyone() {
        let mut last = HashMap::new();
        let out = fold_deltas(
            &mut last,
            &[peer("bb", 100, 200, 1_000)],
            &tags(),
            1_050,
            180,
        );
        assert!(out.is_empty());
    }

    /// Исчезнувшие пиры не копятся в памяти агента месяцами.
    #[test]
    fn vanished_peers_are_dropped_from_the_carry_over() {
        let mut last = HashMap::new();
        fold_deltas(
            &mut last,
            &[peer("aa", 100, 200, 1_000)],
            &tags(),
            1_050,
            180,
        );
        assert_eq!(last.len(), 1);
        fold_deltas(&mut last, &[], &tags(), 1_150, 180);
        assert!(last.is_empty());
    }
}

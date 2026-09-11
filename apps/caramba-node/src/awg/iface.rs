//! Интерфейс `awg0`: запуск процесса, адрес, маршрутизация и NAT.
//!
//! Всё здесь идемпотентно и вызывается каждые две минуты — так же, как уже
//! устроен `sync_firewall` для sing-box. Сеть на узлах разная: на germany и usa
//! стоит iptables, на veles и canada его может не быть вовсе (только nft), и
//! ровно на этом сломалась бы «очевидная» реализация через один iptables.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tracing::{info, warn};

/// Имя таблицы nft, которую заводит агент. Своя таблица, а не вмешательство в
/// чужие цепочки: её можно пересоздать целиком, ничего не сломав вокруг.
pub const NFT_TABLE: &str = "caramba_awg";

/// Чем на этом узле управлять netfilter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetfilterBackend {
    Iptables,
    Nft,
    None,
}

/// Есть ли исполняемый файл в системе.
fn have(binary: &str) -> bool {
    std::process::Command::new("which")
        .arg(binary)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Какой инструмент использовать. iptables предпочтительнее только потому, что
/// им же пользуется уже работающий `sync_firewall`, и правила узла остаются в
/// одном месте.
pub fn detect_backend() -> NetfilterBackend {
    if have("iptables") {
        NetfilterBackend::Iptables
    } else if have("nft") {
        NetfilterBackend::Nft
    } else {
        NetfilterBackend::None
    }
}

/// Запустить `ip` с аргументами; вернуть stdout.
async fn run(program: &str, args: &[&str]) -> Option<std::process::Output> {
    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || std::process::Command::new(program).args(args).output())
        .await
        .ok()?
        .ok()
}

async fn run_ok(program: &str, args: &[&str]) -> bool {
    run(program, args)
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Поднять процесс amneziawg-go.
///
/// Без `-f`: процесс демонизируется сам и переживает перезапуск агента. Это
/// важнее удобства супервизии — узел обновляет себя сам и перезапускается, а
/// рвать при этом туннели всем пользователям недопустимо.
pub async fn spawn_amneziawg(binary: &Path, iface: &str) -> anyhow::Result<()> {
    // Каталоги под сокет создаём заранее: у разных сборок апстрима путь
    // отличается, и процесс не обязан уметь создавать каталог сам.
    for dir in super::uapi::SOCKET_DIRS {
        let _ = tokio::fs::create_dir_all(dir).await;
    }

    let bin = binary.to_path_buf();
    let iface = iface.to_string();
    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&bin)
            .arg(&iface)
            .env("LOG_LEVEL", "error")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    })
    .await??;

    if !status.success() {
        anyhow::bail!("amneziawg-go завершился с кодом {:?}", status.code());
    }
    info!("AWG: процесс amneziawg-go запущен");
    Ok(())
}

/// Погасить интерфейс, когда панель выключила AWG.
///
/// Убиваем именно процесс: `ip link delete` на TUN, которым владеет живой
/// amneziawg-go, оставил бы процесс крутиться без интерфейса.
pub async fn bring_down(iface: &str) {
    let _ = run("pkill", &["-f", &format!("amneziawg-go {iface}")]).await;
    let _ = run("ip", &["link", "delete", "dev", iface]).await;
    for dir in super::uapi::SOCKET_DIRS {
        let _ = tokio::fs::remove_file(format!("{dir}/{iface}.sock")).await;
    }
}

/// Есть ли уже нужный адрес в выводе `ip -o addr show dev …`.
pub fn address_present(ip_output: &str, cidr: &str) -> bool {
    let wanted = cidr.trim();
    ip_output
        .split_whitespace()
        .any(|token| token.trim_end_matches(',') == wanted)
}

/// Назначить адрес интерфейсу, если его там ещё нет.
pub async fn ensure_address(iface: &str, cidr: &str) {
    let existing = run("ip", &["-o", "-4", "addr", "show", "dev", iface])
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    if address_present(&existing, cidr) {
        return;
    }
    if run_ok("ip", &["addr", "add", cidr, "dev", iface]).await {
        info!("AWG: адрес {cidr} назначен интерфейсу {iface}");
    } else {
        warn!("AWG: не удалось назначить адрес {cidr} интерфейсу {iface}");
    }
}

/// Поднять интерфейс и выставить MTU.
pub async fn ensure_up(iface: &str, mtu: u32) {
    let mtu = mtu.to_string();
    let _ = run_ok("ip", &["link", "set", "mtu", &mtu, "up", "dev", iface]).await;
}

/// Включить форвардинг. Без него узел принимает пакеты и молча их выбрасывает:
/// на germany и veles `net.ipv4.ip_forward` равен нулю прямо сейчас.
pub async fn ensure_forwarding() {
    if !run_ok("sysctl", &["-w", "net.ipv4.ip_forward=1"]).await {
        warn!("AWG: не удалось включить net.ipv4.ip_forward — трафик пиров никуда не пойдёт");
    }
}

/// Аргументы одного правила iptables. `op` — `-C` (проверка), `-A`/`-I`.
pub fn iptables_rules(iface: &str, subnet: &str, port: u16) -> Vec<(&'static str, Vec<String>)> {
    let port = port.to_string();
    vec![
        // NAT для всего, что уходит НЕ обратно в туннель: имя внешнего
        // интерфейса на узлах разное, и знать его не требуется.
        (
            "nat",
            vec![
                "POSTROUTING".into(),
                "-s".into(),
                subnet.into(),
                "!".into(),
                "-o".into(),
                iface.into(),
                "-j".into(),
                "MASQUERADE".into(),
            ],
        ),
        (
            "filter",
            vec![
                "FORWARD".into(),
                "-i".into(),
                iface.into(),
                "-j".into(),
                "ACCEPT".into(),
            ],
        ),
        (
            "filter",
            vec![
                "FORWARD".into(),
                "-o".into(),
                iface.into(),
                "-m".into(),
                "state".into(),
                "--state".into(),
                "RELATED,ESTABLISHED".into(),
                "-j".into(),
                "ACCEPT".into(),
            ],
        ),
        (
            "filter",
            vec![
                "INPUT".into(),
                "-p".into(),
                "udp".into(),
                "--dport".into(),
                port,
                "-j".into(),
                "ACCEPT".into(),
            ],
        ),
    ]
}

/// Скрипт для `nft -f -`.
///
/// Пересоздаём таблицу целиком в одной транзакции: это единственный способ
/// сделать набор правил идемпотентным, не разбирая текстовый вывод `nft list`.
/// Пустой `table` перед `delete` нужен, чтобы удаление не падало на узле, где
/// таблицы ещё нет.
pub fn nft_script(iface: &str, subnet: &str, port: u16) -> String {
    format!(
        "table ip {table} {{}}\n\
         delete table ip {table}\n\
         table ip {table} {{\n\
         \tchain postrouting {{\n\
         \t\ttype nat hook postrouting priority srcnat; policy accept;\n\
         \t\tip saddr {subnet} oifname != \"{iface}\" masquerade\n\
         \t}}\n\
         \tchain forward {{\n\
         \t\ttype filter hook forward priority filter; policy accept;\n\
         \t\tiifname \"{iface}\" accept\n\
         \t\toifname \"{iface}\" ct state related,established accept\n\
         \t}}\n\
         \tchain input {{\n\
         \t\ttype filter hook input priority filter; policy accept;\n\
         \t\tudp dport {port} accept\n\
         \t}}\n\
         }}\n",
        table = NFT_TABLE,
        iface = iface,
        subnet = subnet,
        port = port
    )
}

/// Разложить правила NAT/форвардинга/порта на узле.
pub async fn ensure_netfilter(iface: &str, subnet: &str, port: u16) {
    match detect_backend() {
        NetfilterBackend::Iptables => {
            for (table, rule) in iptables_rules(iface, subnet, port) {
                let mut check: Vec<&str> = vec!["-t", table, "-C"];
                check.extend(rule.iter().map(String::as_str));
                if run_ok("iptables", &check).await {
                    continue;
                }
                let mut add: Vec<&str> = vec!["-t", table, "-I"];
                add.extend(rule.iter().map(String::as_str));
                if run_ok("iptables", &add).await {
                    info!(
                        "AWG: правило iptables добавлено ({} {})",
                        table,
                        rule.join(" ")
                    );
                } else {
                    warn!(
                        "AWG: правило iptables не добавлено ({} {})",
                        table,
                        rule.join(" ")
                    );
                }
            }
        }
        NetfilterBackend::Nft => {
            // Уже разложено ровно под эту подсеть и порт — не трогаем.
            let listed = run("nft", &["list", "table", "ip", NFT_TABLE])
                .await
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            if listed.contains(subnet) && listed.contains(&format!("dport {port}")) {
                return;
            }
            let script = nft_script(iface, subnet, port);
            match apply_nft(&script).await {
                Ok(()) => info!("AWG: правила nft разложены для {subnet}"),
                Err(e) => warn!("AWG: правила nft не разложены ({e})"),
            }
        }
        NetfilterBackend::None => {
            warn!("AWG: на узле нет ни iptables, ни nft — NAT для пиров не настроен");
        }
    }
}

async fn apply_nft(script: &str) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).await?;
        stdin.shutdown().await?;
    }
    let out = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await??;
    if !out.status.success() {
        anyhow::bail!("nft: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Открыть UDP-порт AWG там, где фаерволом рулит ufw.
///
/// Отдельно от `ensure_netfilter`: при активном ufw правила пишет он, и это же
/// делает уже существующий `sync_firewall` для портов sing-box.
pub async fn ensure_udp_port(port: u16) {
    let ufw_active = have("ufw")
        && run("ufw", &["status"])
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("Status: active"))
            .unwrap_or(false);
    if ufw_active {
        let rule = format!("{port}/udp");
        if run_ok("ufw", &["allow", &rule]).await {
            info!("AWG: ufw открыл {rule}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Вывод `ip -o -4 addr show` разбирается по токенам: адрес там стоит
    /// после `inet` вместе с маской.
    #[test]
    fn an_existing_address_is_recognised() {
        let out = "12: awg0    inet 10.66.0.1/16 scope global awg0\\       valid_lft forever";
        assert!(address_present(out, "10.66.0.1/16"));
        assert!(!address_present(out, "10.66.0.2/16"));
        // Другая маска — это другой адрес, его нужно назначить заново.
        assert!(!address_present(out, "10.66.0.1/24"));
    }

    /// Пустой вывод — адреса нет, а не «есть».
    #[test]
    fn an_empty_ip_output_means_no_address() {
        assert!(!address_present("", "10.66.0.1/16"));
    }

    /// Правило NAT не должно зависеть от имени внешнего интерфейса: на узлах
    /// он называется по-разному (eth0, ens3, enp1s0).
    #[test]
    fn the_masquerade_rule_avoids_naming_the_wan_interface() {
        let rules = iptables_rules("awg0", "10.66.0.0/16", 51820);
        let (table, rule) = &rules[0];
        assert_eq!(*table, "nat");
        let joined = rule.join(" ");
        assert_eq!(
            joined,
            "POSTROUTING -s 10.66.0.0/16 ! -o awg0 -j MASQUERADE"
        );
    }

    /// Форвардинг в обе стороны и порт — без них пиры подключатся, но трафик
    /// не пойдёт; это самая частая причина «AWG поднялся и не работает».
    #[test]
    fn forwarding_and_the_listen_port_are_covered() {
        let rules = iptables_rules("awg0", "10.66.0.0/16", 51820);
        let all: Vec<String> = rules.iter().map(|(_, r)| r.join(" ")).collect();
        assert!(all.iter().any(|r| r.starts_with("FORWARD -i awg0")));
        assert!(all.iter().any(|r| r.contains("RELATED,ESTABLISHED")));
        assert!(
            all.iter()
                .any(|r| r == "INPUT -p udp --dport 51820 -j ACCEPT")
        );
    }

    /// nft-скрипт обязан пересоздавать таблицу, а не дописывать правила:
    /// иначе каждые две минуты в цепочке появлялся бы дубликат.
    #[test]
    fn the_nft_script_recreates_its_own_table() {
        let script = nft_script("awg0", "10.66.0.0/16", 51820);
        assert!(script.contains(&format!("delete table ip {NFT_TABLE}")));
        assert!(script.contains("ip saddr 10.66.0.0/16 oifname != \"awg0\" masquerade"));
        assert!(script.contains("udp dport 51820 accept"));
        assert!(script.contains("ct state related,established accept"));
        // Пустая таблица перед удалением — иначе на чистом узле nft упадёт.
        assert!(
            script.find(&format!("table ip {NFT_TABLE} {{}}")).unwrap()
                < script
                    .find(&format!("delete table ip {NFT_TABLE}"))
                    .unwrap()
        );
    }

    /// Выбор бэкенда обязан оставаться разрешимым и там, где нет ни одного
    /// инструмента: на таком узле AWG просто не получит NAT, но агент живёт.
    #[test]
    fn a_backend_is_always_decidable() {
        let backend = detect_backend();
        assert!(matches!(
            backend,
            NetfilterBackend::Iptables | NetfilterBackend::Nft | NetfilterBackend::None
        ));
    }
}

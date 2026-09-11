use askama::Result;

// Helper for Rust code (non-template usage)

pub fn format_bytes_str(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// Helper for Askama templates (must match filter signature)
#[allow(dead_code)]
pub fn format_bytes(s: &i64) -> Result<String> {
    Ok(format_bytes_str(*s as u64))
}

/// Глобальный тумблер AmneziaWG (настройка панели `amneziawg_enabled`).
///
/// Раньше это был env `CARAMBA_ENABLE_AMNEZIAWG`, и смысл был обратный:
/// «не пускать AWG-инбаунд в конфиг sing-box, иначе узел ляжет». Теперь AWG на
/// ноде это отдельный процесс amneziawg-go с интерфейсом awg0, sing-box про
/// него ничего не знает, и уронить узел включением нечем. Поэтому тумблер
/// переехал в админку: оператор включает протокол целиком, а конкретные ноды
/// объявляют его своим пер-нодовым тумблером (`node_awg.enabled`).
///
/// Зеркало атомарное, а не запрос к БД, потому что генераторы подписки
/// синхронные. Обновляет его `AwgService::refresh_gate` на каждом heartbeat
/// узла и сразу при сохранении настроек. До первого обновления после старта
/// панели значение консервативное — выключено.
static AMNEZIAWG_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Обновляет зеркало тумблера. Единственный, кто это делает, —
/// `AwgService::refresh_gate`.
pub fn set_amneziawg_enabled(enabled: bool) {
    AMNEZIAWG_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Включён ли AmneziaWG на панели.
pub fn amneziawg_enabled() -> bool {
    AMNEZIAWG_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Тумблер глобальный на процесс, а тесты внутри крейта идут параллельно:
/// без общей блокировки тест, включающий AmneziaWG, ломал бы тест, который
/// проверяет поведение при выключенном.
#[cfg(test)]
pub static GATE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Раньше клиентская эмиссия (mihomo/clash) гейтилась отдельным флагом: узел
/// не мог служить AWG, и надо было уметь отдать прокси клиенту, не ломая
/// конфиг узла. Теперь узел служит AWG по-настоящему, и раздельных состояний
/// больше нет: тумблер один. Функция оставлена как единая точка вызова для
/// клиентских путей (каталог CSM, api/v2/app, генератор clash).
pub fn amneziawg_client_enabled() -> bool {
    amneziawg_enabled()
}

// Askama filters are functions.
// I can define `format_bytes_i64` or just expect i64 since DB uses i64.

pub fn current_panel_version() -> String {
    if let Ok(v) = std::env::var("CARAMBA_VERSION") {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    for path in [
        "/opt/caramba/.caramba-version",
        "/opt/caramba/VERSION",
        ".caramba-version",
    ] {
        if let Ok(raw) = std::fs::read_to_string(path) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    let cargo_version = env!("CARGO_PKG_VERSION");
    if cargo_version.starts_with('v') {
        cargo_version.to_string()
    } else {
        format!("v{}", cargo_version)
    }
}

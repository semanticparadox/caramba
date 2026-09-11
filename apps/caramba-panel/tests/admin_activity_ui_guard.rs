//! Инварианты админки для онлайна, ёмкости и трафика узлов.
//!
//! Живой базы у тестов крейта нет, поэтому проверки идут по тексту — в том же
//! стиле, что `node_activity_sources_guard.rs` и `sql_dialect_guard.rs`.
//!
//! Все отказы, которые здесь ловятся, объединяет одно: страница отрисуется, а
//! число на ней будет не про то. Именно так админка год показывала «ONLINE» по
//! таблице, куда почти ничего не писалось, и «Total Traffic (30d)» с алл-тайм
//! суммой внутри.

use std::fs;
use std::path::{Path, PathBuf};

fn panel_file(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Переносы и отступы в разметке незначимы: сравнивать хочется состав.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Строка узла берёт онлайн из heartbeat, а не из лиз устройств.
///
/// Регрессия, которая не должна вернуться: «ONLINE» считался как
/// `COUNT(DISTINCT subscription_id) FROM subscription_device_leases`, а лизы
/// наполнял опрос Clash API `:9090` с панели. У одного узла порт закрыт
/// хостером снаружи, у hysteria2 в `/connections` поля с пользователем нет
/// вовсе — счётчик показывал ноль там, где люди были, и отличить это от
/// «правда никого» было нельзя.
#[test]
fn servers_page_reads_heartbeat_activity_not_clash_leases() {
    let src = squash(&panel_file("src/handlers/admin/nodes.rs"));

    assert!(
        src.contains("NodeActivityService::new(pool.clone())"),
        "страница узлов перестала брать активность из node_activity_service"
    );
    assert!(
        !src.contains("FROM subscription_device_leases"),
        "счётчик онлайна снова считается по лизам устройств (опрос Clash API)"
    );
    assert!(
        !src.contains("fn fetch_node_metrics"),
        "вернулась старая пара счётчиков subs_map/online_map"
    );
}

/// Три числа в строке узла подписаны по-разному и кликабельны.
///
/// «Сейчас», «Онлайн 15 мин» и «Настроено» считаются по разным источникам и
/// окнам. Прежние подписи PEERS/ONLINE/SUBS не говорили ни об окне, ни об
/// источнике, и оператор складывал их между собой как однородные величины.
#[test]
fn node_row_labels_each_counter_and_links_to_its_list() {
    let html = squash(&panel_file("templates/partials/nodes_rows.html"));

    for label in ["Сейчас", "Онлайн 15 мин", "Настроено"] {
        assert!(html.contains(label), "пропала подпись счётчика `{label}`");
    }
    for kind in ["kind=now", "kind=online", "kind=configured"] {
        assert!(
            html.contains(&format!("/online?{kind}")),
            "число в строке узла перестало открывать список `{kind}`"
        );
    }
    // Тултипы приходят полями из кода: текст, вписанный в шаблон руками,
    // разъедется с окном, по которому число посчитано.
    for tip in ["{{ tip_now }}", "{{ tip_online }}", "{{ tip_configured }}"] {
        assert!(
            html.contains(tip),
            "тултип {tip} больше не приходит из кода"
        );
    }
}

/// Подписи к числам собираются из констант сервиса.
///
/// Если вписать «180 секунд» текстом, окно в коде можно будет поменять, а
/// подпись останется прежней — и разойтись они смогут молча.
#[test]
fn counter_tooltips_are_built_from_service_constants() {
    let src = squash(&panel_file("src/handlers/admin/nodes.rs"));
    let body_start = src
        .find("fn counter_tooltips()")
        .expect("пропала сборка тултипов из констант");
    let body = &src[body_start..body_start + 1200];

    for constant in [
        "NOW_WINDOW_SECS",
        "ONLINE_WINDOW_SECS",
        "CAPACITY_WARN_PERCENT",
        "CAPACITY_CRITICAL_PERCENT",
    ] {
        assert!(
            body.contains(constant),
            "тултипы перестали использовать константу {constant}"
        );
    }
}

/// Шкала заполнения узла рисуется от действующего потолка.
///
/// Потолок ручной (`max_users_override`), иначе расчётный: расчётный
/// перезаписывается телеметрией на каждом heartbeat, поэтому вписать своё
/// число прямо в `max_users` нельзя — его затрёт.
#[test]
fn capacity_bar_uses_effective_limit_and_warns_at_the_top() {
    let html = squash(&panel_file("templates/partials/nodes_rows.html"));
    assert!(
        html.contains("{{ row.bar_percent }}%") && html.contains("{{ row.bar_class }}"),
        "шкала заполнения узла пропала из строки"
    );
    assert!(
        html.contains("нужна новая нода"),
        "пропала подпись про необходимость нового узла при переполнении"
    );
    assert!(
        html.contains("Потолок не задан"),
        "узел без потолка обязан говорить об этом, а не показывать пустую шкалу"
    );

    let src = squash(&panel_file("src/handlers/admin/nodes.rs"));
    assert!(
        src.contains("UPDATE nodes SET max_users_override = $1 WHERE id = $2"),
        "ручной потолок узла больше не сохраняется из формы"
    );
    // Точечные переключатели политик шлют форму без этого поля: запись
    // «нет значения — значит NULL» молча сбрасывала бы настройку оператора.
    assert!(
        src.contains("if let Some(raw) = form.max_users_override.as_ref()"),
        "ручной потолок пишется безусловно и затирается точечными обновлениями"
    );
}

/// Список за числом узла ограничен и честно говорит про обрезку и про отказ.
#[test]
fn node_user_list_is_bounded_and_reports_failures() {
    let src = squash(&panel_file("src/handlers/admin/nodes.rs"));
    assert!(
        src.contains("query.limit.unwrap_or(100).clamp(1, 100)"),
        "лимит списка узла снова не ограничен сверху"
    );
    assert!(
        src.contains("Some(format!( \"Список не получен: {}. Смотрите логи панели.\", e ))")
            || src.contains("Список не получен"),
        "отказ выборки снова показывается как пустой список"
    );

    let html = squash(&panel_file("templates/partials/node_online_list.html"));
    assert!(
        html.contains("{% if let Some(message) = error.as_ref() %}"),
        "партиал списка перестал показывать текст отказа"
    );
    assert!(
        html.contains("список обрезан"),
        "обрезанный список обязан об этом говорить"
    );
}

/// Список Users показывает онлайн и узел без N+1.
///
/// Присутствие берётся одной выборкой на страницу: похода в базу на строку
/// список пользователей не переживёт, а заметно это станет только на проде.
#[test]
fn users_list_resolves_presence_in_one_batch() {
    let src = squash(&panel_file("src/handlers/admin/users.rs"));
    assert!(
        src.contains(".presence_for_users(&ids)"),
        "список Users перестал брать присутствие пакетно"
    );
    // Пустой срез сервис трактует как «все пользователи» — на пустой странице
    // это выборка всей активности вместо ничего.
    assert!(
        src.contains("if users.is_empty() { HashMap::new() }"),
        "пустой список пользователей снова запрашивает присутствие по всей базе"
    );

    let html = squash(&panel_file("templates/users.html"));
    assert!(
        html.contains("{% if let Some(p) = presence.get(&user.id) %}"),
        "колонки «Онлайн» и «Нода» пропали из списка пользователей"
    );
    assert!(
        html.contains("{{ p.node_name }}"),
        "колонка узла больше не показывает имя узла"
    );
}

/// Карточка пользователя показывает присутствие и подневный трафик.
#[test]
fn user_card_shows_presence_and_daily_traffic() {
    let src = squash(&panel_file("src/handlers/admin/users.rs"));
    assert!(
        src.contains("presence_for_user(id)"),
        "карточка пользователя перестала показывать, где он сейчас"
    );
    assert!(
        src.contains("get_user_traffic_history(id, 30)"),
        "карточка пользователя перестала показывать трафик по дням"
    );

    let html = squash(&panel_file("templates/user_details.html"));
    assert!(
        html.contains("userTrafficChart"),
        "пропал график подневного трафика пользователя"
    );
    // Пустой график и «трафика не было» неразличимы, если про пустоту молчать.
    assert!(
        html.contains("Записей о трафике за 30 дней нет"),
        "пустая история трафика снова рисуется как график без данных"
    );
}

/// Подпись «за 30 дней» не стоит рядом с алл-тайм числом без пояснения.
///
/// Раньше оба числа были одним и тем же значением под подписью «30d»: чем
/// дольше жили узлы, тем сильнее врала карточка, и по самой цифре это было
/// не видно.
#[test]
fn traffic_cards_separate_window_from_all_time() {
    for file in ["templates/dashboard.html", "templates/analytics.html"] {
        let html = squash(&panel_file(file));
        assert!(
            html.contains("Трафик за 30 дней"),
            "{file}: 30-дневная карточка перестала называть своё окно"
        );
        assert!(
            html.contains("За всё время:"),
            "{file}: алл-тайм трафик снова показывается без отдельной подписи"
        );
        assert!(
            !html.contains("Total Traffic (30d)") && !html.contains("LIVE 30D VOLUME"),
            "{file}: вернулась старая подпись, не различавшая окно и алл-тайм"
        );
    }
}

/// Маршрут списка узла зарегистрирован.
///
/// Кнопки в строке узла ведут на `/nodes/{id}/online`: без маршрута они молча
/// вернут 404 в модалку, и выглядеть это будет как пустой список.
#[test]
fn node_online_route_is_registered() {
    let src = squash(&panel_file("src/main.rs"));
    assert!(
        src.contains("\"/nodes/{id}/online\"")
            && src.contains("handlers::admin::nodes::get_node_user_list"),
        "маршрут списка пользователей узла не зарегистрирован"
    );
}

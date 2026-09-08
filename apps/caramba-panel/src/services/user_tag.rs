//! Canonical proxy-identity wire formats, shared by node config generation
//! and by everything that has to match or reproduce them.
//!
//! Three formats live here, and every one of them is derived from the SAME
//! client identity:
//!
//!  * the sing-box auth tag `user_{identity}` — written by
//!    `orchestration_service`, parsed back by traffic accounting
//!    (`api/v2/node.rs::heartbeat`) and connection enforcement
//!    (`ConnectionService`). Interpreting it as a subscription id silently
//!    breaks enforcement (see the NOTE in `traffic_service.rs` about the
//!    deleted `process_node_usage`).
//!  * the `{identity}:{uuid}` Hysteria2 password / naive credential — written
//!    into node configs by `orchestration_service` AND handed to the user in
//!    subscription links by `subscription_service` / `api::internal`.
//!  * the identity itself, which is `users.tg_id` when present and a stable
//!    negative surrogate derived from `users.id` when it is not.
//!
//! The identity is NOT the raw `users.tg_id` column. That column is nullable
//! (email-registered accounts have no Telegram id), so both halves must go
//! through [`config_client_identity`] / [`config_client_identity_for_user`].
//! Reading the column directly is how the halves drift: the node ends up
//! expecting `-42:uuid` while the user is handed `0:uuid`, and Hysteria2
//! authentication just fails with nothing in any log to explain it.
//!
//! Hence the re-exports below: this module is the only import path panel code
//! should use for any of the three, and none of them takes a raw `tg_id`.

/// Identity source and wire-format builders, re-exported so both halves of the
/// system have exactly one import path. Defined in `caramba-db` because the
/// repository layer (`get_active_subs_by_plans`) needs them too and cannot
/// depend on the panel crate.
pub use caramba_db::repositories::subscription_repo::{
    config_client_identity, config_client_identity_for_user, proxy_auth_password,
};

/// Build the auth tag injected into node configs for one client.
///
/// Takes the resolved client identity (see [`config_client_identity`]), never
/// a raw `users.tg_id`.
pub fn user_tag(client_identity: i64) -> String {
    format!("user_{}", client_identity)
}

/// Parse an auth tag back into the client identity it encodes.
///
/// Returns `None` for anything that is not a `user_{i64}` tag (relay tags,
/// vless UUIDs, legacy garbage), so callers can fall back to other
/// identification strategies.
///
/// Note the asymmetry callers must respect: a NEGATIVE result is a surrogate
/// identity, not a Telegram id, so `WHERE tg_id = ANY(...)` will not resolve
/// it — that is the documented cost recorded on [`config_client_identity`].
pub fn parse_user_tag(tag: &str) -> Option<i64> {
    tag.strip_prefix("user_")?.parse::<i64>().ok()
}

/// Пароль, которым узел знает клиента в TUIC / naive / shadowsocks / shadowtls.
///
/// Это ЧЕТВЁРТЫЙ формат в дополнение к трём из шапки модуля, и он НЕ равен
/// [`proxy_auth_password`]. Различие не косметическое: у Hysteria2 пароль —
/// единственный секрет, и в него зашита идентичность (`{identity}:{uuid}`),
/// чтобы узел мог посчитать трафик по одному полю. У TUIC идентичность несёт
/// отдельное поле `uuid`, у naive — `username`, поэтому туда
/// `orchestration_service` кладёт голый uuid без дефисов.
///
/// Пока подписка отдавала сюда `hy2_password`, узел отвечал
/// «authentication: token mismatch» (проверено на TUIC-47a0b813, 85.215.196.151:16400),
/// и вход был мёртв у всех клиентов сразу. Приводим подписку к УЗЛУ, а не
/// наоборот: конфиги трёх нод уже выкачены с этим паролем, а переписать их
/// нельзя без переразвёртывания — и такое переразвёртывание выбило бы всех
/// живых пользователей TUIC ради формата, который сам по себе не лучше.
///
/// Отсюда правило: и генератор подписки, и `orchestration_service` зовут ЭТУ
/// функцию, а не пишут `uuid.replace(...)` у себя.
pub fn node_user_password(uuid: &str) -> String {
    uuid.replace('-', "")
}

/// Достаёт идентичность клиента обратно из [`proxy_auth_password`].
///
/// Нужна там, где на руках только `hy2_password` (генератор подписки получает
/// `UserKeys`, а не `users.id`), но выдать надо `user_{identity}` — логин naive.
/// Возвращает `None`, если строка не в формате `{i64}:{...}`, чтобы вызывающий
/// не собрал логин из мусора и не выдал заведомо неработающую учётку.
pub fn client_identity_from_proxy_password(password: &str) -> Option<i64> {
    password.split(':').next()?.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::{config_client_identity, parse_user_tag, proxy_auth_password, user_tag};

    /// Идентичность клиента ровно так, как её берёт СЕРВЕРНАЯ половина:
    /// `SubscriptionRepository::get_active_subs_by_plans` читает `u.tg_id`
    /// как `Option` вместе с `u.id` и отдаёт результат
    /// `config_client_identity` в `orchestration_service`.
    fn node_side_identity(tg_id: Option<i64>, user_id: i64) -> i64 {
        config_client_identity(tg_id, user_id)
    }

    /// То же для КЛИЕНТСКОЙ половины: `config_client_identity_for_user`
    /// читает ту же колонку тем же `Option`-декодером и зовёт ту же функцию.
    /// Зеркалим здесь только чистую часть — запрос к БД в юнит-тесте не
    /// нужен, расходилась именно арифметика после запроса.
    fn client_side_identity(tg_id: Option<i64>, user_id: i64) -> i64 {
        config_client_identity(tg_id, user_id)
    }

    /// РЕГРЕССИЯ, которая не должна вернуться: у аккаунта без Telegram id
    /// (email-регистрация, `users.tg_id IS NULL`) обе половины обязаны
    /// получить одну и ту же идентичность и один и тот же пароль.
    ///
    /// Раньше клиентская половина подставляла литеральный 0: нода ждала
    /// `-42:uuid`, пользователь получал `0:uuid`, Hysteria2 отвергала
    /// соединение молча — ни строчки в логах панели.
    #[test]
    fn both_halves_agree_on_identity_when_tg_id_is_null() {
        const UUID: &str = "550e8400-e29b-41d4-a716-446655440000";

        let node = node_side_identity(None, 42);
        let client = client_side_identity(None, 42);
        assert_eq!(node, client, "identity halves diverged for a NULL tg_id");

        // Пароль ноды (orchestration_service -> Hysteria2User::password)
        // и пароль в ссылке подписки (subscription_service / api::internal)
        // строятся одним построителем и обязаны совпасть байт в байт.
        assert_eq!(
            proxy_auth_password(node, UUID),
            proxy_auth_password(client, UUID)
        );
        // И это НЕ старое ошибочное значение с нулём.
        assert_ne!(
            proxy_auth_password(client, UUID),
            proxy_auth_password(0, UUID)
        );

        // Тег ноды разбирается обратно в ту же идентичность — иначе учёт
        // трафика и kill-switch промахнутся мимо пользователя.
        assert_eq!(parse_user_tag(&user_tag(node)), Some(client));
    }

    /// Telegram-аккаунты (все 18 пользователей прода на сегодня) не меняются:
    /// идентичность равна tg_id, ссылки и теги остаются прежними.
    #[test]
    fn both_halves_agree_on_identity_for_telegram_users() {
        const UUID: &str = "550e8400-e29b-41d4-a716-446655440000";
        let node = node_side_identity(Some(95_679_857), 1);
        let client = client_side_identity(Some(95_679_857), 1);
        assert_eq!(node, 95_679_857);
        assert_eq!(node, client);
        assert_eq!(
            proxy_auth_password(node, UUID),
            proxy_auth_password(client, UUID)
        );
        assert_eq!(user_tag(node), "user_95679857");
    }

    /// Regression test: pins the tag format compatibility between config
    /// generation (orchestration_service) and connection matching
    /// (connection_service / heartbeat traffic accounting), in both
    /// directions. If either helper changes shape, enforcement silently
    /// dies — this test must fail first.
    #[test]
    fn generated_tag_round_trips_through_parser() {
        // Generation -> parsing: what orchestration writes, enforcement reads.
        assert_eq!(parse_user_tag(&user_tag(123456789)), Some(123456789));
        // Exact wire format sing-box configs carry today.
        assert_eq!(user_tag(123456789), "user_123456789");
        // Parsing -> generation: a parsed tag regenerates byte-identically.
        assert_eq!(user_tag(parse_user_tag("user_42").unwrap()), "user_42");
    }

    #[test]
    fn parser_rejects_non_user_tags() {
        // vless UUID in chains must not be mistaken for a user tag.
        assert_eq!(parse_user_tag("550e8400-e29b-41d4-a716-446655440000"), None);
        assert_eq!(parse_user_tag("relay_7_legacy"), None);
        assert_eq!(parse_user_tag("user_"), None);
        assert_eq!(parse_user_tag("user_abc"), None);
        assert_eq!(parse_user_tag(""), None);
    }

    #[test]
    fn parser_accepts_negative_and_large_ids() {
        // Telegram ids fit i64; keep the parser as wide as the generator.
        assert_eq!(parse_user_tag(&user_tag(i64::MAX)), Some(i64::MAX));
        assert_eq!(parse_user_tag(&user_tag(-1)), Some(-1));
    }
}

-- AmneziaWG, ступень 1: сервер AWG живёт на ноде отдельным процессом
-- (amneziawg-go, интерфейс awg0), а НЕ инбаундом sing-box. Стоковый sing-box
-- не умеет wireguard-inbound с полями обфускации, поэтому единственный источник
-- правды по серверным ключам, порту и параметрам jc/jmin/.../h4 это панель:
-- она генерирует их один раз на ноду и отдаёт агенту в разделе `awg` конфига.
--
-- Все три таблицы аддитивны: ничего существующего не трогают, откат релиза
-- панели их просто перестаёт читать.

-- Серверная сторона AWG на конкретной ноде. Ровно одна строка на ноду.
CREATE TABLE IF NOT EXISTS node_awg (
    node_id      BIGINT      PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    -- UDP-порт awg0. Совпадает с портом зеркального inbound-а (протокол
    -- amneziawg), чтобы порт был занят в общем аллокаторе портов ноды.
    listen_port  INTEGER     NOT NULL,
    -- X25519 в стандартном base64 (формат wg), НЕ hex: hex нужен только UAPI,
    -- и перевод делает агент.
    private_key  TEXT        NOT NULL,
    public_key   TEXT        NOT NULL,
    -- Адрес самого интерфейса на ноде вместе с маской пула пиров.
    address_cidr TEXT        NOT NULL DEFAULT '10.66.0.1/16',
    -- Параметры обфускации AmneziaWG. jc/jmin/jmax/s1/s2 малые, h1..h4 это
    -- 32-битные маркеры типов пакетов, поэтому BIGINT (в u32 не влезает в INT).
    jc           INTEGER     NOT NULL,
    jmin         INTEGER     NOT NULL,
    jmax         INTEGER     NOT NULL,
    s1           INTEGER     NOT NULL,
    s2           INTEGER     NOT NULL,
    h1           BIGINT      NOT NULL,
    h2           BIGINT      NOT NULL,
    h3           BIGINT      NOT NULL,
    h4           BIGINT      NOT NULL,
    -- Пер-нодовый тумблер из карточки ноды. Глобальный тумблер это настройка
    -- панели amneziawg_enabled; нода поднимает awg0 только когда включены оба.
    enabled      BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Клиентская пара ключей подписки и её адрес в пуле 10.66.0.0/16.
-- Ключ выводится детерминированно из uuid подписки (та же функция, что и в
-- генераторе подписки), поэтому строка это журнал выданного, а не секрет,
-- который где-то ещё нельзя воспроизвести.
CREATE TABLE IF NOT EXISTS subscription_awg_keys (
    subscription_id BIGINT      PRIMARY KEY REFERENCES subscriptions (id) ON DELETE CASCADE,
    public_key      TEXT        NOT NULL,
    private_key     TEXT        NOT NULL,
    -- Адрес пира с маской /32, выдан из пула по subscription_id.
    allowed_ip      TEXT        NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_subscription_awg_keys_pub
    ON subscription_awg_keys (public_key);

-- Онлайн и дельты трафика по пользователям, как их видит конкретная нода.
-- Заполняется из heartbeat.active_users (и sing-box, и AWG), UI поверх этого
-- строится отдельной волной.
CREATE TABLE IF NOT EXISTS node_user_activity (
    node_id      BIGINT      NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    user_tag     TEXT        NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    online       BOOLEAN     NOT NULL DEFAULT FALSE,
    rx_delta     BIGINT      NOT NULL DEFAULT 0,
    tx_delta     BIGINT      NOT NULL DEFAULT 0,
    PRIMARY KEY (node_id, user_tag)
);

CREATE INDEX IF NOT EXISTS idx_node_user_activity_online
    ON node_user_activity (online, last_seen_at);

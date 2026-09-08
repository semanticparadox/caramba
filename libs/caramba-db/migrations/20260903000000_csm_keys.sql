-- CSM/1: ключи подписи оператора и хранилище подписанных каталогов.
--
-- Спецификация: apps/caramba-client/docs/protocol/02-SPEC.md,
-- 03-WIRE.md 1.5 и 13.8, 01-DECISION.md раздел 7 блок P2.
--
-- Приватные ключи в базе НЕ хранятся. Здесь только публичная часть и учёт:
-- корневой ключ живёт офлайн у оператора, онлайн-ключ панель читает из своего
-- секрета окружения. Это разделение и есть смысл двух ролей: компрометация
-- работающей панели не должна давать возможность подменить якорь доверия.

CREATE TABLE IF NOT EXISTS csm_keys (
    id           BIGSERIAL PRIMARY KEY,
    -- Роль: 1 = root, 2 = online (03-WIRE.md раздел 5).
    role         SMALLINT     NOT NULL,
    -- keyid_trunc = sha256(pk)[0..12], 24 hex-символа. Подсказка для поиска.
    kid          TEXT         NOT NULL,
    -- Публичный ключ Ed25519, 32 байта в hex.
    public_key   TEXT         NOT NULL,
    -- Метка оператора для интерфейса: «ноутбук», «сейф», «прод».
    label        TEXT,
    -- Ключ выведен из обращения: подписи им больше не выпускаются, а клиент
    -- узнаёт об отзыве из списка rev ключевого документа.
    revoked_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- Один и тот же ключ не должен появиться дважды: иначе в ключевом документе
-- окажется дубль kid, а верификатор обязан такой кадр отвергнуть целиком.
CREATE UNIQUE INDEX IF NOT EXISTS idx_csm_keys_kid ON csm_keys (kid);

-- Ровно один активный корневой ключ на инстанс: pid тенанта считается от него,
-- и второй активный корень означал бы вторую личность у той же панели.
CREATE UNIQUE INDEX IF NOT EXISTS idx_csm_keys_single_root
    ON csm_keys ((TRUE)) WHERE role = 1 AND revoked_at IS NULL;

-- Подписанные каталоги по тирам.
--
-- 03-WIRE.md 1.5: подпись детерминирована, но iat и exp входят в payload,
-- поэтому пересборка в другую секунду даёт другой кадр, другой chash и другой
-- cat_id. Значит кадр надо ХРАНИТЬ и переподписывать только когда изменилось
-- содержимое, а не когда пришёл запрос. Ключ содержимого (content_digest)
-- считается по модели тира: узлы, релэи, маршруты, зеркала, DNS, ресурсы,
-- пины и пороги, и ни по чему больше: ни по часам, ни по запросившему.
CREATE TABLE IF NOT EXISTS csm_catalogs (
    id             BIGSERIAL PRIMARY KEY,
    -- Идентификатор тира, 1..1023 (03-WIRE.md 8.1, поправка о диапазоне).
    tier           INTEGER      NOT NULL,
    -- sha256 модели содержимого тира, hex.
    content_digest TEXT         NOT NULL,
    -- Монотонная версия документа.
    ver            BIGINT       NOT NULL,
    -- Момент, когда изменилось СОДЕРЖИМОЕ, а не момент запроса.
    iat            BIGINT       NOT NULL,
    -- sha256 подписанного кадра, hex. Именно он публикуется в tiers ключевого
    -- документа и связывает каталог с корнем.
    chash          TEXT         NOT NULL,
    -- Подписанный кадр каталога целиком.
    frame          BYTEA        NOT NULL,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_csm_catalogs_tier_digest
    ON csm_catalogs (tier, content_digest);
CREATE INDEX IF NOT EXISTS idx_csm_catalogs_chash ON csm_catalogs (chash);

-- Части каталога: клиент забирает каталог кусками, каждый подписан отдельно
-- (03-WIRE.md 8.4). Хранятся вместе с родительским кадром, чтобы отдача была
-- чтением, а не подписанием.
CREATE TABLE IF NOT EXISTS csm_catalog_chunks (
    catalog_id  BIGINT      NOT NULL REFERENCES csm_catalogs (id) ON DELETE CASCADE,
    idx         INTEGER     NOT NULL,
    total       INTEGER     NOT NULL,
    frame       BYTEA       NOT NULL,
    PRIMARY KEY (catalog_id, idx)
);

-- Документы, подписанные КОРНЕВЫМ ключом.
--
-- Корневой приватный ключ живёт офлайн, поэтому работающая панель их не
-- подписывает, а хранит и отдаёт: оператор подписывает документ у себя и
-- импортирует сюда. Именно это разделение делает компрометацию панели
-- восстановимой: злоумышленник с онлайн-ключом не может подменить якорь.
CREATE TABLE IF NOT EXISTS csm_root_documents (
    id         BIGSERIAL   PRIMARY KEY,
    -- Тип документа: 1 = ключевой, 5 = bootstrap, 8 = резервный пул.
    doc_type   SMALLINT    NOT NULL,
    -- Монотонная версия из payload документа.
    ver        BIGINT      NOT NULL,
    -- Подписанный кадр целиком, как его получит клиент.
    frame      BYTEA       NOT NULL,
    -- sha256 кадра, hex: удобно сверять с опубликованным значением.
    frame_hash TEXT        NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Одна версия одного типа импортируется один раз.
CREATE UNIQUE INDEX IF NOT EXISTS idx_csm_root_documents_type_ver
    ON csm_root_documents (doc_type, ver);

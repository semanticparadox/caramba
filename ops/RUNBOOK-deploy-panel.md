# Выкат панели на боевой сервер

Панель на `zeus` одна, без стенда и без канареечного выката, и на ней сидят
платящие подписчики. Перезапуск сервиса — это короткий полный простой. Поэтому
здесь записан не «примерный порядок», а тот, по которому идут.

## Как код вообще попадает на сервер

Rust-тулчейна на `zeus` нет. Единственный путь: тег `v*` → GitHub Actions
(`.github/workflows/release.yml`) собирает musl-бинарники и публикует релиз →
на сервере `caramba upgrade --to vX.Y.Z` их забирает.

Версию релиза Actions берёт из `apps/caramba-panel/Cargo.toml`, а `upgrade`
адресует релиз по тегу. Если они разойдутся, `upgrade --to` не найдёт того, что
собралось. Версии во всех Cargo.toml воркспейса поднимают одним шагом.

## Правило про миграции, из-за которого можно потерять откат

Панель мигрирует базу сама: `libs/caramba-db/src/lib.rs` вызывает
`sqlx::migrate!().run()` при старте и падает, если миграция не прошла. То есть
любой выкат применяет все миграции, которые есть в бинарнике.

Обратной дороги у этого нет. sqlx отказывается стартовать, если в базе есть
применённая миграция, которой нет в бинарнике (`MigrateError::VersionMissing`,
`ignore_missing` нигде не выставлен). Старый бинарник после новой миграции
уходит в краш-луп, а `Restart=always` делает это бесконечным.

Отсюда два следствия:

1. Релиз без новых миграций откатывается одной командой. Релиз с миграциями —
   нет, и это надо знать ДО выката, а не во время аварии.
2. Если откат всё же нужен после миграции — сначала убрать записи о миграциях,
   потом запускать старый бинарник. Таблицы не трогать, они аддитивные:

   ```sql
   DELETE FROM _sqlx_migrations WHERE version IN (20260903000000, 20260903100000);
   ```

Проверить, что релиз действительно без миграций:

```bash
git diff --name-status <предыдущий-тег>..HEAD -- libs/caramba-db/migrations
# пусто = откат остаётся одной командой
```

## Перед выкатом

```bash
# 1. Снимок базы. Делать всегда, даже когда «ничего не трогаем».
ssh zeus 'sudo -u postgres pg_dump -Fc caramba > /root/caramba-pre-<версия>.dump && ls -l /root/caramba-pre-<версия>.dump'

# 2. Копия текущего бинарника рядом. upgrade перезаписывает его атомарно,
#    и без копии откатываться будет не на что, если релиза уже нет.
ssh zeus 'cp -a /opt/caramba/caramba-panel /opt/caramba/caramba-panel.$(cat /opt/caramba/.caramba-version).bak'

# 3. Убедиться, что в релизе есть все ассеты: caramba-panel, caramba-node,
#    caramba-sub, caramba-installer, sing-box, caramba-app-dist.tar.gz, SHA256SUMS.
gh release view v<версия> --json assets --jq '.assets[].name'
```

## Выкат

```bash
ssh zeus 'sudo caramba upgrade --to v<версия>'
ssh zeus 'systemctl is-active caramba-panel; journalctl -u caramba-panel -n 80 --no-pager'
```

Побочный эффект, о котором стоит помнить: `upgrade` заодно подменяет на хосте
sing-box, поэтому «выкат только панели» на самом деле не только панели.

## Проверки после выката — определение готовности

Выкат считается состоявшимся, когда прошли все три. Первые две проверяют, что
бинарник тот; третья — что он делает то, ради чего его катили. Без третьей
получается ровно та ошибка, которая уже случалась: «поставилось» приняли за
«работает».

```bash
# 1. Версия
ssh zeus 'cat /opt/caramba/.caramba-version'

# 2. Миграции: для релиза без миграций число строк обязано не измениться.
printf '%s\n' "SELECT count(*) FROM _sqlx_migrations;" > /tmp/q.sql
ssh zeus 'sudo -u postgres psql -d caramba -At' < /tmp/q.sql

# 3. Функциональная. Завести одноразовый аккаунт в @exa_robot, принять условия,
#    затем убедиться, что подписка не только появилась, но и пригодна к раздаче:
printf '%s\n' "SELECT s.id, s.user_id, p.name, s.status, (s.vless_uuid IS NOT NULL) AS connectable
FROM subscriptions s JOIN plans p ON p.id = s.plan_id
WHERE p.is_free ORDER BY s.id DESC LIMIT 5;" > /tmp/q.sql
ssh zeus 'sudo -u postgres psql -d caramba -At' < /tmp/q.sql
```

Затем реально подключиться этим аккаунтом. Строка в базе ничего не доказывает:
подписка без `vless_uuid` выглядит в базе точно так же и при этом невидима для
всех inbound-ов.

## Бэкфилл

`ops/sql/free-plan-backfill.sql`, только после проверки 3. Сначала прочитать
глазами вывод первого SELECT, потом коммитить вручную.

## Откат

```bash
ssh zeus 'systemctl stop caramba-panel'
# только если релиз нёс миграции — сначала снять записи о них (см. выше)
ssh zeus 'cp -a /opt/caramba/caramba-panel.<старая-версия>.bak /opt/caramba/caramba-panel'
ssh zeus 'systemctl start caramba-panel; journalctl -u caramba-panel -n 50 --no-pager'
```

Дамп базы нужен только если что-то испортило данные. Восстановление дампа —
последнее средство: оно вернёт и те платежи, что прошли после снимка, в
состояние «не было».

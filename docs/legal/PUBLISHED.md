# Где опубликованы юридические документы

Канонические анонимные копии (Telegraph, Instant View внутри Telegram, доступно из РФ):

- Пользовательское соглашение — https://telegra.ph/Polzovatelskoe-soglashenie-08-31-24
- Политика конфиденциальности — https://telegra.ph/Politika-konfidencialnosti-08-31-69

Зеркало на случай проблем с telegra.ph — те же пути на graph.org:
https://graph.org/Polzovatelskoe-soglashenie-08-31-24 · https://graph.org/Politika-konfidencialnosti-08-31-69

Токен редактирования страниц — `TELEGRAPH_TOKEN` в `.env` репозитория webq-hq
(не коммитится). Правка: изменить `docs/legal/*.md` здесь → перепубликовать
через api.telegra.ph `editPage` с этим токеном — страницы обновятся по тем же URL.

Точка синхронизации в продукте: настройка панели `terms_of_service`
(админка → Settings) — короткий текст согласия при первом /start в боте,
должен ссылаться на эти URL. Сами полные тексты в настройку не вставлять
(лимит сообщения Telegram 4096 символов).

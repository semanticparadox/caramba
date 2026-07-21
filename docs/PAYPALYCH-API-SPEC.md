# Paypalych (pal24.pro / pally.info) — actual API spec

> Снято с https://pally.info/reference/api (HTML скопирован в `/Users/smtcprdx/apidocs.md` 2026-07-21).
> Это **единственный** источник правды для провайдера — заменил все
> догадки из PR #1 (commit `475b839`).

---

## 1. Авторизация

```
Authorization: Bearer {token}
```

Токен формата `72|xxxxx...` (Laravel Sanctum). Берётся в личном кабинете pally.info → «API интеграция».

Все эндпоинты ниже живут на **`https://pal24.pro/api/v1/`**.

---

## 2. `POST /api/v1/bill/create` — создать счёт на оплату

### Формат запроса: **multipart/form-data** (НЕ JSON!)

| Поле | Обязательно | Тип | Возможные значения | Пример | Описание |
|---|---|---|---|---|---|
| `amount` | ✅ | decimal | — | `380.99` | Сумма счёта |
| `shop_id` | ✅ | string | — | `LXZv3R7Q8B` | **ID магазина/проекта**. Без него не работают Success/Fail/Result URL |
| `order_id` | ❌ | string | — | `order-285394168` | Наш ID заказа. Вернётся в postback как `InvId` |
| `description` | ❌ | string | — | `Order #285394168` | Описание платежа |
| `type` | ❌ | enum | `normal` / `multi` | `normal` | `normal` = одноразовый, `multi` = многоразовый |
| `locale` | ❌ | enum | `en` / `ru` | `ru` | Язык формы оплаты |
| `currency_in` | ❌ | enum | `RUB` / `USD` / `EUR` | `RUB` | Валюта счёта. По умолчанию — валюта магазина (или RUB) |
| `custom` | ❌ | string | — | `my-custom-string` | Произвольное поле, вернётся в postback |
| `payer_pays_commission` | ❌ | enum | `0` / `1` | `1` | Кто платит комиссию (0 = продавец) |
| `payer_data[phone]` | ❌ | string | — | `+79991234567` | Телефон плательщика |
| `payer_data[email]` | ❌ | string | — | `payer@email.com` | Email плательщика (заполнит поле на форме) |
| `name` | ❌ | string | — | `Donation` | Название ссылки, видно на форме |
| `ttl` | ❌ | integer | — | `600` | Время жизни счёта в **секундах** |
| `return_url` | ❌ | string | URL | — | Кнопка «Назад в магазин». Домен должен матчить домен в настройках магазина |
| `success_url` | ❌ | string | URL | — | Страница успешной оплаты |
| `fail_url` | ❌ | string | URL | — | Страница неуспешной оплаты |
| `payment_method` | ❌ | enum | `BANK_CARD` / `SBP` | `SBP` | Если задан — выбор метода на форме заблокирован |
| `request_fields[email]` | ❌ | boolean | — | `false` | Требовать email на форме |
| `request_fields[phone]` | ❌ | boolean | — | `false` | Требовать телефон |
| `request_fields[name]` | ❌ | boolean | — | `false` | Требовать ФИО |
| `request_fields[comment]` | ❌ | boolean | — | `false` | Требовать комментарий |
| `items` | ❌ | array | — | — | Список товаров (см. `Order Item Resource` в доке) |
| `show_converted_amount` | ❌ | boolean | — | `true` | Показывать конвертированную сумму |

> **Заметки для caramba:**
> - `success_url`/`fail_url`/`return_url` опциональны — по факту они указываются **в настройках магазина** в кабинете pally.info. Если передать здесь — перекроют дефолтные. Удобно: можем НЕ передавать и опираться на дашборд.
> - `shop_id` — единственное обязательное кроме `amount` поле. У нас один проект → один shop_id, передаём всегда.
> - `currency_in: RUB` — у нас всё в рублях.
> - `type: normal` — каждый инвойс одноразовый, для VPN-подписки правильно.
> - `order_id` — наш UUID из `PaymentSession::id`, чтобы в postback'е вернулся как `InvId` и мы связали webhook с нашей сессией.
> - `custom` — можно засунуть plan_id для логирования, вернётся в postback.
> - `description` — пользовательский текст на форме, удобно для бренда.

### Пример запроса

```bash
curl -X POST 'https://pal24.pro/api/v1/bill/create' \
  -H 'Authorization: Bearer 72|oBCB7Z3SmUm1gvkpEdRcSR2q1XTxp36nsM0kUMSu0otSA95' \
  -d 'amount=100.05' \
  -d 'description=Some desc' \
  -d 'order_id=15' \
  -d 'type=multi' \
  -d 'shop_id=LXZv3R7Q8B' \
  -d 'custom=123' \
  -d 'currency_in=RUB'
```

### Формат ответа

```json
{
  "success": "true",      // ← ВНИМАНИЕ: это STRING, не bool!
  "link_url": "https://pally.info/link/3P1p2rgW7Y",
  "link_page_url": "https://pally.info/transfer/3P1p2rgW7Y",
  "bill_id": "3P1p2rgW7Y"
}
```

| Поле | Тип | Описание |
|---|---|---|
| `success` | **string** | `"true"` или `"false"` (НЕ boolean!) |
| `link_url` | URL | Ссылка с QR-кодом |
| `link_page_url` | URL | **Основная ссылка для оплаты** — сюда редиректим пользователя |
| `bill_id` | string | Уникальный ID счёта в Pal24 |

---

## 3. Webhook (postback) — `Result URL` с настройках магазина

Pal24 делает **HTTP POST** на `Result URL` (наш `https://panel.exarobot.top/api/webhooks/payment/paypalych`) с `application/x-www-form-urlencoded` (НЕ JSON!) со следующими полями:

| Поле | Тип | Пример | Описание |
|---|---|---|---|
| `Status` | string | `SUCCESS` | Статус платежа. Другие значения: `FAIL` |
| `InvId` | string | `Заказ 123` | Наш `order_id`, тот что мы передали в bill/create |
| `Commission` | string | `2.54` | Комиссия Pal24 |
| `CurrencyIn` | string | `RUB` | Валюта |
| `OutSum` | string | `18.54` | Сумма к оплате (та же что в amount) |
| `TrsId` | string | `3P1p2rgW7Y` | ID транзакции в Pal24 (совпадает с `bill_id`) |
| `custom` | string | `my_custom_data` | Наш произвольный payload |
| `SignatureValue` | string | `4A41373E24C99A2656D1D306C800B53C` | Подпись |

### Формат подписи

```
strtoupper(md5(OutSum + ":" + InvId + ":" + apiToken))
```

**Не HMAC-SHA256, а plain MD5!**

Где:
- `OutSum` — сумма из payload
- `InvId` — наш `order_id` из payload
- `apiToken` — наш Bearer token (тот же что в `Authorization: Bearer ...`)

Результат: **uppercase hex** (32 символа).

**Алгоритм проверки на нашей стороне:**
1. Достаём из POST-тела `OutSum`, `InvId`, `SignatureValue`.
2. Считаем `expected = strtoupper(md5(OutSum + ":" + InvId + ":" + api_token))`.
3. Сравниваем `expected` с `SignatureValue` (constant-time).

---

## 4. Редирект после оплаты — Success/Fail URL

Pal24 делает **HTTP POST** на `Success URL` / `Fail URL` (тоже `application/x-www-form-urlencoded`):

```json
{
  "OutSum": "18.54",
  "CurrencyIn": "RUB",
  "InvId": "Заказ 123",
  "custom": "my_custom_data",
  "SignatureValue": "5A41374P24C99A2156D1D306C800B53C"
}
```

Подпись — **та же формула**. Это для UX (показать пользователю «оплачено»), не для автоматизации. Реальный автоматизационный сигнал — webhook на Result URL.

---

## 5. Другие эндпоинты (для справки, в текущей итерации не нужны)

| Метод | URL | Назначение |
|---|---|---|
| `GET` | `/api/v1/bill/status?id={bill_id}` | Статус конкретного счёта |
| `GET` | `/api/v1/bill/payments` | Список выплат (cursor) |
| `GET` | `/api/v1/bill/search?start_date=&finish_date=&order_id=` | Поиск счетов по диапазону дат / order_id |
| `POST` | `/api/v1/bill/toggle_activity` | Активировать/деактивировать счёт |
| `GET` | `/api/v1/merchant/balance` | Баланс мерчанта |
| `GET` | `/api/v1/payment/search?start_date=&finish_date=&bill_order_id=` | Поиск платежей |
| `GET` | `/api/v1/payment/status?id={payment_id}` | Статус конкретного платежа |
| `POST` | `/api/v1/payout/cancel` | Отмена выплаты |
| `GET` | `/api/v1/payout/dictionaries/sbp_banks` | Справочник банков СБП |
| `POST` | `/api/v1/payout/personal/create` | Создать выплату на карту/СБП |

Из них нам потенциально полезен `GET /api/v1/bill/status` — для **polling-fallback** в `check_status` (сейчас у нас заглушка, возвращает `"pending"`). Без polling'а webhook-loss означает задержку активации до следующего ретрая от Pal24 (он ретраит ~3 раза за час, если 503/timeout) и ручного recheck'а.

---

## 6. Ошибки

Спека упоминает секцию «Возможные ошибки» для каждого эндпоинта, но полный список не скопирован. Из наблюдений в коде и поиске:

- `401` — невалидный токен
- `4xx` с `error_code` + `error_message` в JSON — валидационные ошибки (неверный `shop_id`, `amount` ≤ 0 и т.д.)
- `5xx` — серверные ошибки, ретраить

---

## 7. Тарифы (комиссии Pal24, согласно roadmap'у)

| Канал | Комиссия | Минимум | Максимум |
|---|---|---|---|
| СБП | 6.5% + 2₽ | 10₽ | 50 000₽ |
| USDT TRC20 (в ₽) | 3% + 1 USDT | 400₽ | 1 000 000₽ |

Канал выбирается клиентом **на стороне Pal24** (если мы не передали `payment_method`). Нам как мерчанту без разницы — мы получаем `SUCCESS` или `FAIL`, деньги одинаково зачисляются на наш баланс.

---

## 8. Что поменять в caramba-панели vs текущая (неправильная) реализация

| Что | Текущий код (`paypalych.rs`) | Правильно |
|---|---|---|
| Формат запроса bill/create | JSON (`.json(&body)`) | **form-urlencoded** (`.form(&body)`) |
| Поле валюты | `currency: "RUB"` | `currency_in: "RUB"` |
| Поле типа счёта | (нет) | `type: "normal"` |
| `success_url` / `fail_url` / `hook_url` / `expire` в запросе | Передаются в API | **Не передаём** (настраиваются в дашборде) |
| `name` / `description` | `description` есть, `name` нет | `description` для формы, `name` тоже передаём |
| Парсинг ответа | `success: Some(bool)` (boolean) | `success: Some(String)` и сравнивать с `"true"` |
| Поле ссылки | `link_page_url` или `link` или `id` | **Только `link_page_url`** (поле `link` — НЕ существует; `id` НЕ возвращается в этом эндпоинте) |
| Webhook body format | JSON | **form-urlencoded** |
| Поля webhook | `status`, `order_id` (lowercase) | **`Status`, `InvId`** (PascalCase) |
| Сигнатура webhook | Header `Sign`/`Signature`/`X-Sign` (HMAC-SHA256 hex) | **Body `SignatureValue`**, plain **MD5** от `OutSum:InvId:apiToken` uppercase hex |
| Секрет для подписи | `paypalych_webhook_secret` | **Сам API token** (Bearer token используется как ключ) |
| Статусы | `paid` / `canceled` / `expired` / `failed` | **`SUCCESS`** → Completed, **`FAIL`** → Failed |
| Idempotency | Нет | Наш `order_id` = UUID платежа, `lookup_session_by_external` уже работает |

---

*Spec version: Pal24 API v1, captured 2026-07-21. Если Pal24 поменяет — обновить здесь и в `paypalych.rs`.*

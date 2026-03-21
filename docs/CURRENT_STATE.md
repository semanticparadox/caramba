# Caramba — Current State (v0.9.14)

Дата / Date: March 2026

---

## RU

### Модули

| Модуль | Роль | Стек |
| --- | --- | --- |
| `apps/caramba-panel` | Панель управления, API, оркестрация | Rust, Axum, Askama/HTMX, SQLx, Redis |
| `apps/caramba-bot` | Telegram бот | Rust, Teloxide |
| `apps/caramba-app` | Telegram Mini App | React, TypeScript |
| `apps/caramba-node` | Агент на VPN-нодах | Rust |
| `apps/caramba-sub` | Сервис подписок | Rust, Axum |
| `apps/caramba-installer` | CLI установщик | Rust |
| `libs/caramba-db` | Модели, репозитории, миграции | SQLx (PostgreSQL) |
| `libs/caramba-shared` | Общие типы и контракты | Rust |

### Реализованные возможности (v0.9.5 -- v0.9.14)

#### Telegram Mini App (TMA)
- Редизайн: тема sunset, компактный hero-блок, QR-коды для подключения.
- Управление устройствами: список подключённых устройств, отключение через API.
- Гайд по подключению для разных клиентов (Hiddify, V2rayNG, Shadowrocket и др.).
- Fallback на `full_name`, если `first_name` не доступно; удалён сломанный выбор страны.

#### Платежи
- Идемпотентность платежей (дедупликация при повторных callback-ах).
- Провайдеры: Cryptomus, NowPayments, Telegram Stars, Lava, AAIO.

#### Авто-relay по гео
- Relay-ноды автоматически подбираются по стране клиента.
- Пользовательские названия серверов (user-friendly naming).
- Relay-ноды скрыты из пользовательского интерфейса.

#### Админ-бот
- Команды: `/stats`, `/gift`, `/promo`, `/ban`.
- Локализация бота (RU/EN).

#### Уведомления
- Система уведомлений для администраторов (платежи, статус нод).

#### Реферальная программа
- Настраиваемый бонус (дни подписки) за приведённого пользователя.

#### Панель администратора
- UI ёмкости серверов (capacity bars).
- Фильтр по странам.
- Кнопка Edit на странице ноды для настройки relay_id.
- Реальные графики аналитики.

#### Генерация конфигураций
- Компактные теги подключений (без названия страны, сокращённые протоколы).
- Пропуск gRPC транспорта для sing-box конфигов (совместимость).
- Восстановлена поддержка gRPC для V2Ray/Clash.
- Исправлен geosite rule, вызывавший краш sing-box 1.8+.

#### Протоколы
VLESS (Reality, WS, HTTPUpgrade, gRPC), Hysteria2, TUIC, Shadowsocks, NaiveProxy, VMess, Trojan, AmneziaWG.

#### Ноды
- Heartbeat с телеметрией (latency, CPU, RAM, скорость).
- SNI-сканирование и управление (pin/block).
- Блокировка торрентов по умолчанию для новых нод.
- Kill Switch и Decoy traffic.

---

## EN

### Modules

| Module | Role | Stack |
| --- | --- | --- |
| `apps/caramba-panel` | Control plane, API, orchestration | Rust, Axum, Askama/HTMX, SQLx, Redis |
| `apps/caramba-bot` | Telegram bot | Rust, Teloxide |
| `apps/caramba-app` | Telegram Mini App | React, TypeScript |
| `apps/caramba-node` | VPN node agent | Rust |
| `apps/caramba-sub` | Subscription service | Rust, Axum |
| `apps/caramba-installer` | CLI installer | Rust |
| `libs/caramba-db` | Models, repositories, migrations | SQLx (PostgreSQL) |
| `libs/caramba-shared` | Shared types and contracts | Rust |

### Implemented Features (v0.9.5 -- v0.9.14)

#### Telegram Mini App (TMA)
- Redesign: sunset theme, compact hero block, QR codes for connection.
- Device management: list connected devices, kick via API.
- Multi-client connect guide (Hiddify, V2rayNG, Shadowrocket, etc.).
- Fallback to `full_name` when `first_name` is unavailable; removed broken country picker.

#### Payments
- Payment idempotency (deduplication on repeated callbacks).
- Providers: Cryptomus, NowPayments, Telegram Stars, Lava, AAIO.

#### Auto-relay by geo
- Relay nodes are automatically selected based on client country.
- User-friendly server naming.
- Relay nodes hidden from user-facing UI.

#### Admin bot
- Commands: `/stats`, `/gift`, `/promo`, `/ban`.
- Bot localization (RU/EN).

#### Notifications
- Notification system for admins (payments, node status).

#### Referral program
- Configurable bonus (subscription days) for referred users.

#### Admin panel
- Server capacity UI (capacity bars).
- Country filter.
- Edit button on node detail page for relay_id configuration.
- Real analytics charts.

#### Config generation
- Compact connection tags (no country name, shortened protocols).
- Skip gRPC transport for sing-box configs (compatibility).
- Restored gRPC support for V2Ray/Clash.
- Fixed geosite rule that crashed sing-box 1.8+.

#### Protocols
VLESS (Reality, WS, HTTPUpgrade, gRPC), Hysteria2, TUIC, Shadowsocks, NaiveProxy, VMess, Trojan, AmneziaWG.

#### Nodes
- Heartbeat with telemetry (latency, CPU, RAM, speed).
- SNI scanning and management (pin/block).
- Torrent blocking enabled by default for new nodes.
- Kill Switch and Decoy traffic.

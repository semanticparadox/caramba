# Caramba

---

## RU

### Caramba — VPN-панель с Telegram Mini App

Комплексное решение для управления VPN-сервисом: панель администратора, Telegram бот, Mini App для пользователей, автоматическая генерация конфигураций sing-box/V2Ray/Clash.

### Возможности

#### Для пользователей
- Telegram Mini App для управления подпиской
- Автоматический подбор серверов по геолокации
- QR-код и ссылка для импорта в VPN-клиент
- Управление подключёнными устройствами (просмотр и отключение)
- Поддержка клиентов: Hiddify, V2rayNG, NekoBox, Shadowrocket, Sing-box и др.
- Реферальная программа с настраиваемым бонусом

#### Для администраторов
- Веб-панель с аналитикой и управлением
- Telegram бот с админ-командами (`/stats`, `/gift`, `/promo`, `/ban`)
- Авто-relay: relay-ноды автоматически подбираются по стране пользователя
- Управление нодами, планами, промокодами
- Мониторинг: трафик, устройства, ёмкость серверов
- Уведомления о платежах и статусе нод

#### Протоколы
VLESS (Reality, WS, HTTPUpgrade, gRPC), Hysteria2, TUIC, Shadowsocks, NaiveProxy, VMess, Trojan, AmneziaWG

#### Архитектура
- **caramba-panel** — основной сервер (Axum, PostgreSQL, Redis)
- **caramba-bot** — Telegram бот (Teloxide)
- **caramba-app** — Telegram Mini App (React, TypeScript)
- **caramba-node** — агент на VPN-нодах
- **caramba-sub** — сервис подписок
- **caramba-installer** — установщик

#### Быстрый старт

```bash
curl -fsSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh | sudo bash
```

Установщик (`caramba` CLI) — единая точка входа для установки, обновления, диагностики, бэкапа и удаления.

#### Режимы развёртывания

| Режим | Топология | Для чего |
| --- | --- | --- |
| Hub | Panel + Sub (+ Bot) на одном хосте | быстрый запуск, тесты, небольшие инсталляции |
| Distributed | Panel на управляющем хосте, Sub/Bot/Node на отдельных | продакшен, изоляция, масштабирование |

#### Разработка

```bash
cargo check --workspace
cargo test --workspace
cd apps/caramba-app && npm run build
```

Запуск сервисов локально:

```bash
cargo run -p caramba-panel
cargo run -p caramba-sub
cargo run -p caramba-bot
cargo run -p caramba-node
```

#### Документация

- `docs/API.md` — API-эндпоинты
- `docs/CONFIGURATION.md` — параметры конфигурации
- `docs/DATABASE.md` — схема БД
- `docs/DEPLOYMENT.md` — развёртывание
- `docs/DEVELOPMENT.md` — разработка
- `docs/MODULES.md` — модули системы

---

## EN

### Caramba — VPN Panel with Telegram Mini App

A complete VPN service management solution: admin panel, Telegram bot, Mini App for users, automatic config generation for sing-box/V2Ray/Clash.

### Features

#### For users
- Telegram Mini App for subscription management
- Automatic server selection by geolocation
- QR code and link for import into VPN clients
- Device management (list and kick connected devices)
- Client support: Hiddify, V2rayNG, NekoBox, Shadowrocket, Sing-box, etc.
- Referral program with configurable bonus

#### For administrators
- Web panel with analytics and management
- Telegram bot with admin commands (`/stats`, `/gift`, `/promo`, `/ban`)
- Auto-relay: relay nodes are automatically selected based on user's country
- Node, plan, and promo code management
- Monitoring: traffic, devices, server capacity
- Payment and node status notifications

#### Protocols
VLESS (Reality, WS, HTTPUpgrade, gRPC), Hysteria2, TUIC, Shadowsocks, NaiveProxy, VMess, Trojan, AmneziaWG

#### Architecture
- **caramba-panel** — main server (Axum, PostgreSQL, Redis)
- **caramba-bot** — Telegram bot (Teloxide)
- **caramba-app** — Telegram Mini App (React, TypeScript)
- **caramba-node** — agent on VPN nodes
- **caramba-sub** — subscription service
- **caramba-installer** — installer

#### Quick Start

```bash
curl -fsSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh | sudo bash
```

The installer (`caramba` CLI) is the single entrypoint for install, upgrade, diagnostics, backup, and uninstall.

#### Deployment Modes

| Mode | Topology | Best for |
| --- | --- | --- |
| Hub | Panel + Sub (+ Bot) on one host | quick launch, testing, small deployments |
| Distributed | Panel on control host, Sub/Bot/Node on separate hosts | production, isolation, scaling |

#### Development

```bash
cargo check --workspace
cargo test --workspace
cd apps/caramba-app && npm run build
```

Run services locally:

```bash
cargo run -p caramba-panel
cargo run -p caramba-sub
cargo run -p caramba-bot
cargo run -p caramba-node
```

#### Repository Layout

```
apps/caramba-panel      — admin UI, APIs, orchestration
apps/caramba-node       — node agent
apps/caramba-sub        — subscription/frontend worker
apps/caramba-bot        — Telegram bot worker
apps/caramba-installer  — installer and upgrade tooling
apps/caramba-app        — Mini App frontend (React/TS)
libs/caramba-db         — DB models, repositories, migrations
libs/caramba-shared     — shared contracts and config types
```

#### Documentation

- `docs/API.md` — API endpoints
- `docs/CONFIGURATION.md` — configuration reference
- `docs/DATABASE.md` — database schema
- `docs/DEPLOYMENT.md` — deployment guide
- `docs/DEVELOPMENT.md` — development guide
- `docs/MODULES.md` — system modules

---

## CI/CD

Release workflow: `.github/workflows/release.yml` — runs on tags matching `v*` or manual dispatch.

## License

Until a formal `LICENSE` file is added, repository content should be treated as source-available / all-rights-reserved by default.

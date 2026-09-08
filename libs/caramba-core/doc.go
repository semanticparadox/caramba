// Package carambacore — корень общего клиентского ядра caramba.
//
// caramba-core — «мозг», разделяемый между CLI (caramba-cli) и
// Flutter-приложением (через gomobile/FFI). Он отвечает за:
//
//   - auth         — аутентификацию в панели (email/пароль, Telegram), хранение
//     и авто-обновление JWT;
//   - subscription — загрузку mihomo (clash.meta) конфигурации подписки;
//   - profile      — наложение локальной политики (TUN, kill-switch,
//     split-tunnel, DNS) на конфиг панели;
//   - engine       — абстракцию VPN-движка поверх mihomo (нативное ядро
//     подключается build-тегом `mihomo`);
//   - api          — тонкий фасад (Login/Logout/Status/Up/Down), который
//     одинаково вызывают CLI и мобильные привязки.
//
// Зависимости намеренно минимальны: только стандартная библиотека и
// gopkg.in/yaml.v3.
package carambacore

/*
 * caramba_core.h — C ABI десктопного ядра caramba (cgo c-shared).
 *
 * Этот заголовок описывает экспортируемую поверхность libcaramba_core
 * (libcaramba_core.so / .dylib / caramba_core.dll), которую десктопный Flutter-
 * плагин (Linux/macOS/Windows) вызывает через dart:ffi. Сигнатуры совпадают с
 * //export-функциями в ffi/ffi.go.
 *
 * cgo генерирует собственный заголовок при `-buildmode=c-shared`; этот файл —
 * каноничная, вычищенная копия для вендоринга плагином (см.
 * scripts/build-desktop-lib.sh). Держите его синхронным с ffi/ffi.go.
 *
 * Контракт совпадает с мобильным (gomobile mobile.Client):
 *   - stage: "disconnected" | "connecting" | "connected" | "reconnecting" | "error";
 *   - статус (CarambaStatus): {"stage":...,"detail":...,"connectedSinceMs":...,
 *     "mode":"tun"|"proxy","mixedPort":7890} (mixedPort только в proxy-режиме);
 *   - трафик (CarambaTraffic): {"downBps":...,"upBps":...,"downTotal":...,"upTotal":...}.
 *
 * Владение памятью:
 *   - Каждая функция, возвращающая char*, отдаёт строку, выделенную в Go. Её
 *     ОБЯЗАН освободить вызывающий через CarambaFreeString ровно один раз.
 *   - Функции, документированные как «NULL при успехе», на успехе возвращают
 *     NULL, а на ошибке — char* с JSON {"error":"..."} (тоже освобождается
 *     CarambaFreeString).
 *   - Хэндл (long) создаётся CarambaNew (>0) и освобождается CarambaFree.
 *     Неизвестный хэндл → JSON-ошибка.
 *
 * TUN на десктопе: fd НЕ передаётся (tunFd=-1) — mihomo сам поднимает TUN
 * (wintun/utun/tun). Требуются привилегии; на Windows рядом нужен wintun.dll.
 *
 * Режим без привилегий: CarambaSetTunnelMode(h, "proxy", 7890) убирает TUN и
 * поднимает вместо него локальный mixed-инбаунд (SOCKS5+HTTP) на 127.0.0.1:7890.
 * Это позволяет доказать реальное соединение по подписке без root; трафик в
 * порт направляет приложение или системный прокси ОС.
 */

#ifndef CARAMBA_CORE_H
#define CARAMBA_CORE_H

#ifdef __cplusplus
extern "C" {
#endif

/* Создать ядро. panelURL обязателен; остальное может быть "" или NULL.
 * Возвращает хэндл >0 либо 0 при ошибке. */
extern long CarambaNew(char *panelURL, char *subURL, char *workDir, char *tokenPath);

/* Инъекция JWT приложения + UUID подписки (шов аутентификации).
 * NULL при успехе, иначе JSON-ошибка. */
extern char *CarambaConfigure(long h, char *panelURL, char *subscriptionID, char *accessToken);

/* Импорт сырой подписки (raw-путь): format = "auto"|"clash"|"singbox"|"v2ray"|
 * "uri". После него CarambaUp(h, "") поднимает туннель из импортированного
 * конфига без панели и без входа. Возвращает JSON метаданных либо JSON-ошибку. */
extern char *CarambaImportSubscription(long h, char *raw, char *format);

/* Переключить способ захвата трафика.
 * mode = "tun" (или "" / NULL) — системный TUN-инбаунд, требует прав
 *        (root/CAP_NET_ADMIN, админ на Windows, Network Extension на Apple);
 * mode = "proxy" — локальный mixed-инбаунд (SOCKS5+HTTP) на 127.0.0.1:port
 *        БЕЗ каких-либо привилегий; трафик в него направляет приложение или
 *        системный прокси ОС.
 * port <= 0 оставляет порт по умолчанию (7890) и значим только в proxy-режиме.
 * Применяется при следующем CarambaUp. NULL при успехе, иначе JSON-ошибка. */
extern char *CarambaSetTunnelMode(long h, char *mode, int port);

/* Проброс TUN fd (на десктопе обычно не нужен; -1 = ядро поднимает TUN само).
 * NULL при успехе, иначе JSON-ошибка. */
extern char *CarambaSetTunFd(long h, int fd);

/* Поднять туннель. serverID может быть "" (выбор панели).
 * Возвращает JSON UpResult либо JSON-ошибку. */
extern char *CarambaUp(long h, char *serverID);

/* Остановить туннель. NULL при успехе, иначе JSON-ошибка. */
extern char *CarambaDown(long h);

/* Плоский статус {stage,detail?,connectedSinceMs} для status-канала. */
extern char *CarambaStatus(long h);

/* Плоские счётчики {downBps,upBps,downTotal,upTotal} для traffic-канала. */
extern char *CarambaTraffic(long h);

/* Погасить туннель и освободить ядро по хэндлу. Безопасно при неизвестном хэндле. */
extern void CarambaFree(long h);

/* Освободить строку, возвращённую любой Caramba*-функцией. */
extern void CarambaFreeString(char *s);

#ifdef __cplusplus
}
#endif

#endif /* CARAMBA_CORE_H */

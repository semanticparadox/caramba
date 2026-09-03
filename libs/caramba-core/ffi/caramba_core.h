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
 *     "activeProxy":"NL-1","mode":"tun"|"proxy","mixedPort":7890}
 *     (activeProxy — только при поднятом туннеле, mixedPort — только в
 *     proxy-режиме);
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
 * конфига без панели и без входа. Возвращает JSON метаданных либо JSON-ошибку.
 *
 * Массив servers метаданных — контракт списка узлов для приложения:
 *   {"servers":[{"id":"NL-1","name":"NL-1","type":"vless","server":"1.2.3.4",
 *                "port":443,"country":"NL"}]}
 * где id — имя прокси; именно его передают обратно в CarambaUp как serverID. */
extern char *CarambaImportSubscription(long h, char *raw, char *format);

/* Политика подключения одной JSON-строкой. Все поля опциональны, неизвестные
 * ключи игнорируются:
 *   {"protocol":"auto|AmneziaWG|VLESS-Reality|Hysteria2|TUIC|Shadowsocks",
 *    "preset":"ru-smart|ru-full|telegram-only|ir-smart|by-smart|cn-smart|
 *              streaming|adblock|global|",
 *    "relay":"TR|KZ|FI|",
 *    "stack":"gvisor|system|mixed",
 *    "mtu":1280, "ipv6":false, "fakeIp":true, "killSwitch":true,
 *    "dns":{"nameservers":[...],"fallback":[...]},
 *    "split":{"mode":"off|bypass|allow","apps":[...],"bypassDomains":[...]}}
 * Недопустимое значение перечислимого поля → JSON-ошибка с именем поля, при
 * этом политика НЕ меняется. Применяется при следующем CarambaUp: если туннель
 * уже поднят, приложение обязано переподключиться (CarambaDown + CarambaUp).
 * NULL при успехе, иначе JSON-ошибка. */
extern char *CarambaSetPolicy(long h, char *jsonStr);

/* Замер задержки до каждого узла ТЕКУЩЕЙ загруженной конфигурации
 * (импортированной подписки либо последнего загруженного профиля панели) БЕЗ
 * подъёма туннеля. Возвращает
 *   {"servers":[{"id":"NL-1","name":"NL-1","type":"vless","server":"1.2.3.4",
 *                "port":443,"country":"NL","latencyMs":42}]}
 * где latencyMs = -1 означает «узел не ответил за timeoutMs». Если ничего не
 * загружено — {"servers":[]}. timeoutMs <= 0 — таймаут по умолчанию (3000).
 * Замеры идут параллельно, не более 8 одновременно. Способ замера зависит от
 * сборки библиотеки: TCP-соединение без тега mihomo, настоящий URL-тест через
 * прокси (с TCP-фолбэком) под -tags mihomo. Владелец освобождает строку. */
extern char *CarambaProbe(long h, int timeoutMs);

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

/* Поднять туннель. serverID может быть "" (выбор панели/автоматика). Для
 * импортированной подписки непустой serverID — это ИМЯ узла (поле id из
 * метаданных CarambaImportSubscription); узел закрепляется выбором по умолчанию
 * в селекторе CARAMBA. Возвращает JSON UpResult либо JSON-ошибку. */
extern char *CarambaUp(long h, char *serverID);

/* Остановить туннель. NULL при успехе, иначе JSON-ошибка. */
extern char *CarambaDown(long h);

/* Плоский статус {stage,detail?,connectedSinceMs,activeProxy?} для
 * status-канала. */
extern char *CarambaStatus(long h);

/* Плоские счётчики {downBps,upBps,downTotal,upTotal} для traffic-канала. */
extern char *CarambaTraffic(long h);

/* Погасить туннель и освободить ядро по хэндлу. Безопасно при неизвестном хэндле. */
extern void CarambaFree(long h);

/* Освободить строку, возвращённую любой Caramba*-функцией. */
extern void CarambaFreeString(char *s);

/* --- CSM/1: подписанный манифест и лестница транспортов (ABI v3) ---
 *
 * Каждый символ принимает и отдаёт JSON строкой, как CarambaSetPolicy.
 * Дополнение аддитивное: клиент, собранный против старой библиотеки, не
 * находит символ и деградирует до "CSM недоступен в этой сборке", как это уже
 * происходит с setPolicy и probe. */

/* Регистрация из bootstrap blob либо из origin, кода и пина.
 * Вход {origin?,code?,link_pin?,blob_b64?,subscription_domain?,account_jwt?}.
 * Выход: снимок проверенного состояния или {"error":...}. */
extern char *CarambaCsmEnroll(long h, char *jsonStr);

/* Один цикл выборки документов. Отказ НЕ означает потерю конфигурации:
 * профиль остаётся на кешированных документах и продолжает подключать. */
extern char *CarambaCsmRefresh(long h, int timeoutSec);

/* Личность оператора, состояние проверки документов, отпечаток подписавшего,
 * битовое поле возможностей, возраст конфигурации и её источник. */
extern char *CarambaCsmState(long h);

/* Все скомпилированные ступени с порядком, переключателем и причиной
 * недоступности, плюс история попыток. Недоступная ступень видна и выключена
 * с причиной, а не скрыта. */
extern char *CarambaCsmLadder(long h);

/* Переключатели и порядок ступеней от пользователя.
 * Вход {order?:[int],enabled?:{"5":false},proxy?,tunnel_proxy?}.
 * Ступени 0 и 6 выключить нельзя, попытка возвращает ошибку. */
extern char *CarambaCsmSetLadder(long h, char *jsonStr);

/* Изменение настроек как подписанный запрос; ответ принимается как новая
 * директива. Вход {want?:{"1":"..."},sel?:{...},account_jwt?}. */
extern char *CarambaCsmRequestSettings(long h, char *jsonStr);

/* Переключение хранилища CSM на профиль (02-SPEC.md 1.2: хранилище состояния
 * профиля ОБЯЗАНО ключеваться по pid). Ключ это [a-z0-9_-] длиной до 64;
 * пустой означает единственное хранилище в рабочем каталоге. */
extern char *CarambaCsmSelectProfile(long h, char *key);

/* Ответ пользователя на карточку смены набора rule-set и geo-файлов
 * (02-SPEC.md 7.7.1). Вход {"accept":bool}, выход {"answered":bool}.
 * Пока ответа нет, ядро удерживает ПРЕЖНИЙ набор, поэтому "оставить прежние"
 * действительно оставляет прежние, а не только закрывает карточку. */
extern char *CarambaCsmAnswerCatalogChange(long h, char *jsonStr);

/* Адрес служебного mixed-инбаунда на петле вместе с парой логин-пароль
 * текущего подъёма (socks5://user:pass@127.0.0.1:port), или пустая строка,
 * когда движок не поднят. Нужен обвязке, которая держит ОТДЕЛЬНОЕ ядро под
 * профиль CSM: у того ядра своя лестница и Up на нём никто не зовёт. */
extern char *CarambaLoopbackProxyURL(long h);

/* Личность устройства: завести или отдать уже заведённую (идемпотентно).
 * Вход  {"purpose":"sign"|"agree","require_hardware":true}.
 * Выход {"spki_b64","agree_pub_b64","dtp_hex","tier":1|2|3,"generation":n}.
 * tier: 1 Secure Enclave, 2 StrongBox или TEE, 3 программное хранилище.
 * Десктопная сборка хранилища не имеет и возвращает 3, называя вещи своими
 * именами, а не выдавая программный ключ за аппаратный. */
extern char *CarambaDeviceKeygen(long h, char *jsonStr);

/* Подпись СООБЩЕНИЯ (не дайджеста) ключом подписи устройства.
 * Вход  {"message_b64"}.
 * Выход {"sig_b64","proof_header"} — 64 байта r || s, s в нижней половине
 * порядка, НЕ ASN.1 DER; proof_header это та же подпись как base64url без
 * дополнения, 86 символов, готовое значение X-CSM-Proof (03-WIRE.md 13.6). */
extern char *CarambaDeviceSign(long h, char *jsonStr);

/* ECDH ключом согласования устройства.
 * Вход  {"rkv":n,"peer_pub_b64","kdf_info_b64"} — rkv 0 означает текущее
 * поколение, kdf_info пусто для CSM/1.
 * Выход {"shared_b64","own_pub_b64"} — 32 байта общей координаты X и 65 байт
 * собственного открытого ключа этого поколения (он входит в kem_context
 * DHKEM и известен только держателю ключа). */
extern char *CarambaDeviceAgree(long h, char *jsonStr);

/* Произвольный HTTP запрос через лестницу.
 * Вход {method,path,origin?,headers?,body_b64?,timeout_ms?,rungs?}.
 * Выход {status,headers,body_b64,rung,error?}. */
extern char *CarambaLadderRequest(long h, char *jsonStr);

#ifdef __cplusplus
}
#endif

#endif /* CARAMBA_CORE_H */

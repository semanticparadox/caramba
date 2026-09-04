/// Контракт VPN-движка: стадии, снимки статуса/трафика и абстракция
/// [VpnConnection], которую реализуют все три пути — канальный
/// (`MethodChannelVpnConnection`), внутрипроцессный dart:ffi
/// (`FfiVpnConnection`) и мок (`MockVpnConnection`).
///
/// Контракт живёт в плагине, а не в приложении, чтобы FFI-реализация могла его
/// реализовать без обратной зависимости плагин -> приложение. Модель сервера
/// принадлежит приложению (`Server` из `caramba_client`), поэтому она входит
/// параметром типа `S`: плагин про неё ничего не знает и никогда её не
/// конструирует сам — за это отвечают [VpnServerDescriptor] и
/// [VpnRawTargetFactory], которые передаёт приложение.
library;

import 'dart:async';
import 'dart:typed_data';

import 'package:caramba_vpn/src/core_models.dart';
import 'package:caramba_vpn/src/csm_device.dart';
import 'package:caramba_vpn/src/core_policy.dart';

/// Фаза туннеля — state-машина из DESIGN.md §4
/// (disconnected -> connecting -> connected -> error, + reconnecting).
enum VpnStage { disconnected, connecting, connected, reconnecting, error }

/// Поля узла, которые нативная сторона получает при `connect`.
///
/// Плагин не знает тип модели сервера приложения, поэтому приложение отдаёт
/// маппер [VpnServerDescriptor], сводящий свою модель к этим трём полям.
class VpnServerArgs {
  /// serverId уходит на провод СТРОКОЙ (нативный мост читает его как String).
  final String id;
  final String name;
  final String? countryCode;

  const VpnServerArgs({required this.id, required this.name, this.countryCode});
}

/// Сводит модель сервера приложения к [VpnServerArgs].
typedef VpnServerDescriptor<S extends Object> =
    VpnServerArgs Function(S server);

/// Строит синтетический «сервер» для отображения rawSub-профиля: у
/// импортированной подписки нет узла из `GET /servers`, но пайплайн статуса
/// несёт модель сервера. Приложение отдаёт фабрику по имени профиля.
typedef VpnRawTargetFactory<S extends Object> = S Function(String label);

/// Снимок состояния VPN-соединения, который эмитит ядро и потребляет UI.
///
/// `S` — модель сервера приложения (см. [VpnConnection]).
class VpnStatus<S extends Object> {
  final VpnStage stage;

  /// Сервер, к которому идёт/состоялось подключение (если применимо).
  final S? server;

  /// Под-сообщение для орба («Securing tunnel», причина ошибки и т.п.).
  final String? detail;

  /// Момент перехода в `connected` — UI отсчитывает от него таймер аптайма.
  final DateTime? connectedSince;

  /// Способ захвата трафика, о котором отчиталось ядро (`tun` / `proxy`).
  /// null — ядро поле не прислало (старый бинарь или мок).
  final TunnelMode? mode;

  /// Порт локального mixed-инбаунда; значим только в [TunnelMode.proxy].
  final int? mixedPort;

  /// Имя узла, на который сейчас указывает селектор CARAMBA (ABI v2,
  /// присылается ядром в connected).
  final String? activeProxy;

  const VpnStatus({
    required this.stage,
    this.server,
    this.detail,
    this.connectedSince,
    this.mode,
    this.mixedPort,
    this.activeProxy,
  });

  const VpnStatus.disconnected()
    : stage = VpnStage.disconnected,
      server = null,
      detail = null,
      connectedSince = null,
      mode = null,
      mixedPort = null,
      activeProxy = null;

  bool get isConnected => stage == VpnStage.connected;
  bool get isBusy =>
      stage == VpnStage.connecting || stage == VpnStage.reconnecting;

  /// Парсинг события из нативного канала (или из StatusJSON ядра, разобранного
  /// в такую же карту). `stage` приходит строкой, совпадающей с именами
  /// [VpnStage] (lowerCamel), что задаёт Go-фасад.
  ///
  /// ABI v2 добавляет опциональные `mode`, `mixedPort`, `activeProxy` — они
  /// разбираются мягко: отсутствие или чужой тип дают null, а не исключение.
  factory VpnStatus.fromMap(Map<Object?, Object?> map, {S? server}) {
    final rawProxy = map['activeProxy'];
    return VpnStatus<S>(
      stage: stageFromWire(map['stage'] as String?),
      server: server,
      detail: map['detail'] as String?,
      connectedSince: () {
        final ms = map['connectedSinceMs'];
        if (ms is num && ms > 0) {
          return DateTime.fromMillisecondsSinceEpoch(ms.toInt());
        }
        return null;
      }(),
      mode: TunnelMode.fromWire(map['mode'] as String?),
      mixedPort: (map['mixedPort'] as num?)?.toInt(),
      activeProxy: rawProxy is String && rawProxy.isNotEmpty ? rawProxy : null,
    );
  }

  /// Строка стадии -> [VpnStage]; всё неизвестное считается `disconnected`.
  static VpnStage stageFromWire(String? s) {
    switch (s) {
      case 'connecting':
        return VpnStage.connecting;
      case 'connected':
        return VpnStage.connected;
      case 'reconnecting':
        return VpnStage.reconnecting;
      case 'error':
        return VpnStage.error;
      case 'disconnected':
      default:
        return VpnStage.disconnected;
    }
  }

  VpnStatus<S> copyWith({
    VpnStage? stage,
    S? server,
    String? detail,
    DateTime? connectedSince,
    TunnelMode? mode,
    int? mixedPort,
    String? activeProxy,
  }) => VpnStatus<S>(
    stage: stage ?? this.stage,
    server: server ?? this.server,
    detail: detail ?? this.detail,
    connectedSince: connectedSince ?? this.connectedSince,
    mode: mode ?? this.mode,
    mixedPort: mixedPort ?? this.mixedPort,
    activeProxy: activeProxy ?? this.activeProxy,
  );
}

/// Мгновенная пропускная способность и накопленные счётчики туннеля.
class TrafficStats {
  /// Скорость скачивания, байт/с.
  final int downBps;

  /// Скорость отдачи, байт/с.
  final int upBps;

  /// Всего скачано за сессию, байт.
  final int downTotal;

  /// Всего отдано за сессию, байт.
  final int upTotal;

  const TrafficStats({
    this.downBps = 0,
    this.upBps = 0,
    this.downTotal = 0,
    this.upTotal = 0,
  });

  static const TrafficStats zero = TrafficStats();

  factory TrafficStats.fromMap(Map<Object?, Object?> map) => TrafficStats(
    downBps: (map['downBps'] as num?)?.toInt() ?? 0,
    upBps: (map['upBps'] as num?)?.toInt() ?? 0,
    downTotal: (map['downTotal'] as num?)?.toInt() ?? 0,
    upTotal: (map['upTotal'] as num?)?.toInt() ?? 0,
  );
}

/// Данные для авторизации Go-ядра перед поднятием туннеля — ВСЯ сессия, а не
/// один её короткоживущий кусок.
///
/// Ядро (caramba-core) имеет собственный auth-store и само тянет mihomo-конфиг
/// по подписке, поэтому ему нужно передать сессию, UUID подписки и базовый URL
/// панели. Источник значений — TokenStore (JWT-пара) и `GET /subscription`.
///
/// Здесь долго ездил ОДИН access-токен. Он живёт ~15 минут, ядру нечем было его
/// продлить, и через четверть часа любой его запрос к панели получал 401 без
/// пути назад — при том, что приложение всё это время держало в secure storage
/// вполне живой refresh. Поэтому [refreshToken] и [accessExpiry] едут вместе с
/// токеном: продлевать сессию обязано то, что переживает выгрузку приложения,
/// а это ядро (Android-сервис перезапускается системой в пустом процессе,
/// iOS-расширение — вообще другой процесс без Dart).
class VpnConfig {
  /// Базовый URL панели (`kApiBaseUrl`), без `/api/v2/app`.
  final String panelUrl;

  /// UUID подписки — основа config-URL, который ядро тянет с caramba-sub.
  final String subscriptionUuid;

  /// JWT access-токен текущей сессии (ядро переиспользует его, без ре-логина).
  final String accessToken;

  /// Долгоживущий refresh той же сессии (~30 дней). Пусто — режим деградации:
  /// ядро проработает до истечения [accessToken] и честно перестанет считаться
  /// авторизованным, а не будет упираться в неустранимый 401.
  final String refreshToken;

  /// Когда истекает [accessToken]. `null` — «неизвестно»; ядро тогда возьмёт
  /// срок из claim `exp` самого JWT.
  final DateTime? accessExpiry;

  const VpnConfig({
    required this.panelUrl,
    required this.subscriptionUuid,
    required this.accessToken,
    this.refreshToken = '',
    this.accessExpiry,
  });

  /// Аргументы канала `configure`. Ключ подписки — `subscriptionUuid`
  /// (нативные стороны принимают и устаревший `subscriptionId`).
  ///
  /// Срок уходит целым числом unix-секунд (`0` — неизвестен), а не DateTime:
  /// провод общий для пяти мостов, а plist/SharedPreferences/EncodableValue
  /// одинаково понимают только примитивы.
  Map<String, Object?> toArgs() => <String, Object?>{
    'panelUrl': panelUrl,
    'subscriptionUuid': subscriptionUuid,
    'accessToken': accessToken,
    'refreshToken': refreshToken,
    'accessExpiryUnix': accessExpiryUnix,
  };

  /// Срок жизни access-токена в unix-секундах; `0` — неизвестен.
  int get accessExpiryUnix =>
      accessExpiry == null ? 0 : accessExpiry!.millisecondsSinceEpoch ~/ 1000;

  /// Можно ли вообще конфигурировать ядро (обязательные поля заполнены).
  ///
  /// [refreshToken] сюда НЕ входит: у профиля, заведённого до этой версии, его
  /// в снимке нет, и отказаться конфигурировать ядро вовсе было бы хуже, чем
  /// сконфигурировать его на 15 минут.
  bool get isComplete =>
      panelUrl.isNotEmpty &&
      subscriptionUuid.isNotEmpty &&
      accessToken.isNotEmpty;

  /// Равенство по значению нужно мостам: они пропускают повторный `configure`,
  /// когда шов не изменился. Пока сравнение писалось руками поле за полем,
  /// добавленное поле молча выпадало из него — и ротация токена переставала
  /// доезжать до ядра.
  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is VpnConfig &&
          other.panelUrl == panelUrl &&
          other.subscriptionUuid == subscriptionUuid &&
          other.accessToken == accessToken &&
          other.refreshToken == refreshToken &&
          other.accessExpiryUnix == accessExpiryUnix;

  @override
  int get hashCode => Object.hash(
    panelUrl,
    subscriptionUuid,
    accessToken,
    refreshToken,
    accessExpiryUnix,
  );
}

/// Резолвер конфигурации ядра: лениво (на момент connect) достаёт свежие
/// JWT + UUID подписки + URL панели. Возвращает `null`, если данных пока нет
/// (не залогинены / подписка не загрузилась) — тогда configure пропускается.
typedef VpnConfigResolver = Future<VpnConfig?> Function();

/// Абстракция VPN-движка. За ней стоит Go-ядро caramba-core (mihomo): через
/// gomobile на мобильных, через `libcaramba_core` по dart:ffi на macOS/desktop,
/// либо мок в dev-сборках.
///
/// Контракт намеренно узкий — UI и `vpnProvider` зависят только от него.
abstract interface class VpnConnection<S extends Object> {
  /// Поток снимков состояния. Должен эмитить текущее состояние новому
  /// подписчику (broadcast + кэш последнего значения).
  Stream<VpnStatus<S>> get status;

  /// Поток статистики трафика (~1 тик/с в connected, иначе нули/тишина).
  Stream<TrafficStats> get traffic;

  /// Последнее известное состояние (синхронно, для первичного рендера).
  VpnStatus<S> get currentStatus;

  /// Поднять туннель к [server] (путь panelAccount): ядро авторизуется
  /// (`configure`) и поднимает туннель к выбранному узлу подписки.
  Future<void> connect(S server);

  /// Поднять туннель из импортированной raw-подписки (профиль типа rawSub).
  ///
  /// [raw] в формате [format] (`auto`/`clash`/`singbox`/`v2ray`/`uri`)
  /// передаётся ядру, которое парсит его в mihomo-конфиг и поднимает туннель
  /// без обращения к панели. [label] — только для отображения.
  ///
  /// [serverId] (ABI v2) прикрепляет селектор CARAMBA к конкретному узлу
  /// импортированного конфига (это `id` из [ImportedServer]); null/пусто —
  /// автоматический выбор.
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  });

  /// Разобрать сырую подписку и вернуть её метаданные БЕЗ поднятия туннеля.
  ///
  /// Нужен generic-режиму: список узлов показывается до подключения, а сам
  /// туннель поднимается потом через [connectRaw] с выбранным `serverId`.
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  });

  /// Замерить задержку всех узлов текущего конфига (импортированного или
  /// панельного) без поднятия туннеля. `latencyMs == -1` — таймаут.
  Future<List<ProbeResult>> probe({Duration timeout});

  /// Применить политику ядра (протокол/пресет/relay/stack/DNS/split/...).
  /// Действует со следующего поднятия туннеля.
  Future<void> setPolicy(CorePolicy policy);

  /// Переключить способ захвата трафика. [mixedPort] значим только для
  /// [TunnelMode.proxy]. Действует со следующего поднятия туннеля.
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort});

  // --- CSM/1, ABI v3 --------------------------------------------------------
  //
  // Управляющий слой на Dart НЕ ОТКРЫВАЕТ собственных сокетов к оператору.
  // Иначе регистрация, вход, обновление токена и настройки обходят лестницу
  // транспортов, и приложение вырождается в ступень R0, пока ядро бодро лезет
  // по лестнице за конфигурацией, которую ему больше нечем изменить
  // (02-SPEC.md 8.9). Поэтому запись настроек это вызов ядра, а не HTTP отсюда.

  /// Завести или получить уже заведённую личность устройства.
  ///
  /// Идемпотентно: повторный вызов отдаёт тот же `dtp`. Новая личность на
  /// каждый запуск означала бы новое устройство в списке оператора после
  /// каждого запуска приложения.
  ///
  /// [requireHardware] это ПРОСЬБА, а не требование: сборка без Secure Enclave
  /// и StrongBox вернёт [CsmDeviceKey.hardwareTier] равным 3, и это честный
  /// ответ, а не отказ.
  Future<CsmDeviceKey> deviceKeygen({bool requireHardware});

  /// Подписать СООБЩЕНИЕ (не дайджест) ключом подписи устройства.
  ///
  /// Возвращает ровно 64 байта `r || s` с `s` в нижней половине порядка.
  /// Сообщение собирает вызывающий по 03-WIRE.md 13.6; хеширует его сама
  /// операция подписи на платформе.
  Future<Uint8List> deviceSign(Uint8List message);

  /// ECDH ключом согласования устройства поколения [rkv] (0 — текущее).
  ///
  /// Нужен для распечатывания запечатанной директивы `0x06` там, где закрытый
  /// ключ живёт в аппаратном хранилище и скаляр наружу не выходит.
  Future<CsmAgreement> deviceAgree({
    required Uint8List peerPublicKey,
    int rkv,
    Uint8List? kdfInfo,
  });

  /// Отправить изменение настроек как подписанный запрос и принять подписанный
  /// ответ как новую директиву.
  ///
  /// [want] и [sel] это карты номера поля директивы к значению; отсутствующий
  /// ключ означает «без изменений», текст `default` — сброс к значению
  /// оператора (02-SPEC.md 7.5).
  ///
  /// Значения [want] ТИПИЗИРОВАНЫ так же, как карта `pol` директивы: `String`,
  /// `int`, `bool` или `List<String>`. Ключ, отправленный не своим типом, будет
  /// отвергнут разборщиком панели, и выглядеть это будет как отказ записи
  /// вообще, а не как неверная кодировка одного поля. Возвращает JSON снимка состояния CSM после
  /// применения проверенного ответа. Ядро само подписывает тело, ставит
  /// `X-CSM-Proof` и `If-Match`, идёт по лестнице и ПРОВЕРЯЕТ ответ до того,
  /// как что-либо применить.
  ///
  /// [accountJwt] можно не передавать: ядро возьмёт действующий токен из
  /// своего auth-store.
  Future<String> csmRequestSettings({
    required Map<int, Object?> want,
    Map<int, String> sel,
    String accountJwt,
  });

  /// Снимок проверенного состояния CSM ядра как JSON.
  ///
  /// Читающий вызов: он ничего не запрашивает по сети и ничего не применяет.
  /// Помимо личности оператора и версий документов несёт ПРОЕКЦИЮ доверенного
  /// каталога `resources` и `routes`: имена и подписанные sha256 записей `rs`
  /// и `geo` плюс список `rs` каждого маршрута. Без них клиенту нечем заметить
  /// сужение защиты, которое приходит в каталоге, а не в настройке
  /// (02-SPEC.md 7.7.1).
  ///
  /// Возвращает пустую строку, когда ядро CSM недоступно в этой сборке.
  Future<String> csmState();

  /// Что ПОСЛЕДНИЙ подъём реально применил к маршрутизации, как JSON.
  ///
  /// Читающий вызов: он ничего не запрашивает по сети и ничего не применяет.
  ///
  /// Экран настроек до этого моста не имел НИ ОДНОГО способа отличить
  /// работающий блок рекламы от включённого и мёртвого, и владелец сказал об
  /// этом ровно так: «непонятно, работают или нет». Причина не в экране —
  /// канала не было:
  ///
  ///  * сборка конфига выбрасывает недоступный внешний список ВМЕСТЕ с
  ///    правилами, которые на него ссылались, и «Россия (умный)» без
  ///    `ru-blocked` снаружи неотличим от полноценного;
  ///  * пресеты «Только блок рекламы» и «Стриминг и AI» это ЧИСТЫЕ теги
  ///    GEOSITE, а секция `geox-url` пишется только под доверенным каталогом с
  ///    подписанными хешами. Без базы `GeoSite.dat` такой тег не значит ничего.
  ///
  /// Форма ответа:
  ///
  /// ```json
  /// {
  ///   "known": true,
  ///   "reason": "",
  ///   "detail": "",
  ///   "raised_at_ms": 1756800000000,
  ///   "tunnel_up": true,
  ///   "source": "preset|custom|core_default",
  ///   "preset": {
  ///     "preset_id": "ru-smart", "preset_name": "…", "emoji": "🇷🇺",
  ///     "country": "RU", "final_action": "DIRECT",
  ///     "rules": 12, "dropped_rules": 2,
  ///     "rules_by_type": {"GEOSITE": 7, "GEOIP": 3},
  ///     "geosite_tags": ["telegram", "…"],
  ///     "sources": [{
  ///       "name": "ru-blocked",
  ///       "state": "file|mirror|dropped",
  ///       "reason": "not_in_catalog|no_mirror",
  ///       "detail": "…", "url": "…", "path": "…",
  ///       "rules": 1, "kept_rules": 0
  ///     }]
  ///   },
  ///   "rules": 12,
  ///   "geosite": {
  ///     "required": true, "tags": ["category-ads-all"],
  ///     "state": "not_required|verified|present|refused|unknown",
  ///     "reason": "geox_unmanaged|not_in_catalog|file_missing",
  ///     "detail": "…", "path": "…/GeoSite.dat", "size_bytes": 0, "url": ""
  ///   },
  ///   "relay": {
  ///     "requested": "TR",
  ///     "state": "not_requested|ignored|sent",
  ///     "capability": {"name": "relay_chaining", "supported": false,
  ///                    "reason": "raw_profile", "detail": "…"},
  ///     "dialer_proxy_seen": false, "detail": "…"
  ///   },
  ///   "ignored": [{"name": "…", "supported": false, "reason": "…"}]
  /// }
  /// ```
  ///
  /// Правила чтения, без которых экран снова начнёт врать:
  ///
  ///  * `known: false` означает, что подъёма ещё не было. Это НЕ «всё хорошо».
  ///  * `rules` равен `null`, когда состав правил выбрало само ядро
  ///    (`source: "core_default"`). Ноль и «не знаю» здесь разные ответы.
  ///  * `geosite.state` равен `unknown` вполне законно. Показывать его
  ///    неизвестным, а не зелёным, — единственное честное поведение;
  ///    `refused` же означает, что все правила GEOSITE ТОЧНО мертвы.
  ///  * `relay.dialer_proxy_seen` это НАБЛЮДЕНИЕ над применённым телом
  ///    конфига, а не обещание панели: `state: "sent"` вместе с
  ///    `dialer_proxy_seen: false` значит «страна ушла в панель, цепочки в
  ///    теле нет, подтвердить нечем».
  ///  * `reason` это машинный код; текст пользователю выбирает приложение.
  ///    `detail` написан по-английски для журнала и показу не подлежит.
  ///
  /// Возвращает пустую строку, когда ядро недоступно в этой сборке.
  Future<String> routeReport();

  /// Состояние всех ступеней лестницы и история попыток как JSON.
  ///
  /// История ЛОКАЛЬНА: она никогда не уходит оператору (02-SPEC.md 7.10). Этот
  /// вызов поднимает её из ядра в приложение, потому что экран транспортов
  /// обязан показывать каждую попытку с исходом и причиной (INV-17), а без
  /// моста он показывает пустой список и тем самым врёт.
  ///
  /// Возвращает пустую строку, когда ядро CSM недоступно в этой сборке.
  Future<String> csmLadder();

  /// Зарегистрировать профиль у оператора: bootstrap blob либо origin с кодом
  /// и продиктованным пином (02-SPEC.md 9).
  ///
  /// Регистрация идёт ЧЕРЕЗ ЯДРО и по лестнице транспортов: это единственный
  /// момент, когда доверие СОЗДАЁТСЯ, и открывать под него собственный сокет
  /// из Dart значило бы завести путь к оператору, который лестницу не видит.
  ///
  /// Вход: `{"origin","code","link_pin","blob_b64","subscription_domain",
  /// "account_jwt"}`. Возвращает JSON снимка проверенного состояния: из него
  /// приложение берёт `pid`, отпечаток корня и временной пол и закрепляет
  /// профиль. Без этого моста профиль НИКОГДА не выходит из стадии `pinned`,
  /// а всё, что зависит от проверенного каталога, остаётся выключенным.
  Future<String> csmEnroll({
    String origin,
    String code,
    String linkPin,
    String blobB64,
    String subscriptionDomain,
    String accountJwt,
  });

  /// Один цикл выборки документов: директива, при необходимости каталог.
  ///
  /// Отказ НЕ означает потерю конфигурации: профиль остаётся на кешированных
  /// документах и продолжает подключать (INV-16). Возвращает JSON снимка.
  Future<String> csmRefresh({int timeoutSec});

  /// Применить порядок и переключатели ступеней к ЯДРУ.
  ///
  /// Лестницей ходит ядро, и порядок оно берёт из своего хранилища. Запись,
  /// оставшаяся в слое Dart, меняет только картинку: пользователь переставляет
  /// ступени, экран показывает новый порядок, а выборка идёт по старому.
  ///
  /// [order] это номера ступеней; ступень 0 всегда переносится в начало.
  /// [enabled] это номер ступени к состоянию; 0 и 6 выключить нельзя, и ядро
  /// отвечает на это ошибкой, а не молчанием. [proxy] задаёт адрес
  /// пользовательского прокси ступени R5, пустая строка её снимает.
  Future<void> csmSetLadder({
    List<int> order,
    Map<int, bool> enabled,
    String? proxy,
  });

  /// Ответ пользователя на карточку смены набора rule-set и geo-файлов
  /// (02-SPEC.md 7.7.1, INV-22).
  ///
  /// Пока ответа нет, ядро держит в силе ПРЕЖНИЙ набор. Ответ, оставшийся в
  /// слое Dart, не откатывал бы ничего: ресурсы грузит ядро, и новый каталог
  /// вступал бы в силу к моменту, когда карточка появляется на экране.
  Future<void> csmAnswerCatalogChange({required bool accept});

  /// Переключить хранилище CSM на профиль [profileKey].
  ///
  /// 02-SPEC.md 1.2: каждое хранилище состояния профиля ОБЯЗАНО ключеваться по
  /// `pid`. Одно хранилище на приложение означало бы, что закреплённый корень,
  /// регистрация устройства, монотонные отметки и история попыток второго
  /// оператора ложатся поверх первого.
  ///
  /// Ключ это локальный стабильный идентификатор профиля из `[a-z0-9_-]`,
  /// не длиннее 64 символов. Пустая строка возвращает единственное хранилище в
  /// рабочем каталоге ядра (установки, заведённые до второго оператора).
  Future<void> csmSelectProfile(String profileKey);

  /// Опустить туннель.
  Future<void> disconnect();

  /// Освободить ресурсы (каналы/контроллеры/ядро).
  Future<void> dispose();
}

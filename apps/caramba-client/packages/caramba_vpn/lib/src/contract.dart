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

import 'package:caramba_vpn/src/core_models.dart';
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
typedef VpnServerDescriptor<S extends Object> = VpnServerArgs Function(S server);

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

/// Данные для одноразовой авторизации Go-ядра перед поднятием туннеля.
///
/// Ядро (caramba-core) имеет собственный auth-store и само тянет mihomo-конфиг
/// по подписке, поэтому ему нужно передать JWT-доступ, UUID подписки и базовый
/// URL панели. Источник значений — TokenStore (JWT) и `GET /subscription`.
class VpnConfig {
  /// Базовый URL панели (`kApiBaseUrl`), без `/api/v2/app`.
  final String panelUrl;

  /// UUID подписки — основа config-URL, который ядро тянет с caramba-sub.
  final String subscriptionUuid;

  /// JWT access-токен текущей сессии (ядро переиспользует его, без ре-логина).
  final String accessToken;

  const VpnConfig({
    required this.panelUrl,
    required this.subscriptionUuid,
    required this.accessToken,
  });

  /// Аргументы канала `configure`. Ключ подписки — `subscriptionUuid`
  /// (нативные стороны принимают и устаревший `subscriptionId`).
  Map<String, Object?> toArgs() => <String, Object?>{
    'panelUrl': panelUrl,
    'subscriptionUuid': subscriptionUuid,
    'accessToken': accessToken,
  };

  /// Можно ли вообще конфигурировать ядро (все поля заполнены).
  bool get isComplete =>
      panelUrl.isNotEmpty &&
      subscriptionUuid.isNotEmpty &&
      accessToken.isNotEmpty;
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

  /// Опустить туннель.
  Future<void> disconnect();

  /// Освободить ресурсы (каналы/контроллеры/ядро).
  Future<void> dispose();
}

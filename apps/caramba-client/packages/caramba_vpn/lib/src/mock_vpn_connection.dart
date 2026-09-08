/// Имитация ядра для dev-сборок без нативных артефактов.
library;

import 'dart:async';
import 'dart:typed_data';

import 'package:caramba_vpn/src/contract.dart';
import 'package:caramba_vpn/src/core_models.dart';
import 'package:caramba_vpn/src/core_policy.dart';
import 'package:caramba_vpn/src/csm/crypto/p256.dart';
import 'package:caramba_vpn/src/csm/crypto/sha2.dart';
import 'package:caramba_vpn/src/csm_device.dart';

/// Имитация ядра для desktop/dev: проходит реальный жизненный цикл состояний
/// и генерирует «дышащий» трафик, чтобы UI работал end-to-end без нативного
/// бэка.
///
/// Generic-методы ABI v2 отвечают правдоподобными заглушками: импорт ничего не
/// парсит и отдаёт три выдуманных узла, probe отвечает фиксированными
/// задержками через 300 мс, setPolicy/setTunnelMode лишь запоминают последнее
/// значение (доступно через [lastPolicy] / [lastMode] / [lastMixedPort]).
class MockVpnConnection<S extends Object> implements VpnConnection<S> {
  final _statusCtrl = StreamController<VpnStatus<S>>.broadcast();
  final _trafficCtrl = StreamController<TrafficStats>.broadcast();

  final VpnRawTargetFactory<S>? _rawTarget;

  VpnStatus<S> _last = VpnStatus<S>.disconnected();
  Timer? _trafficTimer;
  Timer? _phaseTimer;
  int _seed = 0;

  /// Последняя политика, переданная в [setPolicy] (null — не вызывали).
  CorePolicy? lastPolicy;

  /// Последний режим захвата трафика из [setTunnelMode].
  TunnelMode lastMode = TunnelMode.tun;

  /// Последний порт mixed-инбаунда из [setTunnelMode].
  int lastMixedPort = 7890;

  MockVpnConnection({VpnRawTargetFactory<S>? rawTarget})
    : _rawTarget = rawTarget;

  /// Выдуманные узлы, которые возвращает [importSubscription] и [probe].
  static const List<ImportedServer> mockServers = <ImportedServer>[
    ImportedServer(
      id: 'NL-01',
      name: 'Amsterdam 01',
      type: 'vless',
      server: 'nl-01.example',
      port: 443,
      country: 'NL',
    ),
    ImportedServer(
      id: 'DE-02',
      name: 'Frankfurt 02',
      type: 'hysteria2',
      server: 'de-02.example',
      port: 8443,
      country: 'DE',
    ),
    ImportedServer(
      id: 'TR-03',
      name: 'Istanbul 03',
      type: 'ss',
      server: 'tr-03.example',
      port: 8388,
      country: 'TR',
    ),
  ];

  @override
  Stream<VpnStatus<S>> get status async* {
    yield _last;
    yield* _statusCtrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic => _trafficCtrl.stream;

  @override
  VpnStatus<S> get currentStatus => _last;

  void _emit(VpnStatus<S> s) {
    _last = s;
    _statusCtrl.add(s);
  }

  @override
  Future<void> connect(S server) async {
    _phaseTimer?.cancel();
    _emit(
      VpnStatus<S>(
        stage: VpnStage.connecting,
        server: server,
        detail: 'Securing tunnel',
      ),
    );
    _phaseTimer = Timer(const Duration(milliseconds: 1400), () {
      _emit(
        VpnStatus<S>(
          stage: VpnStage.connected,
          server: server,
          connectedSince: DateTime.now(),
          mode: lastMode,
          mixedPort: lastMode == TunnelMode.proxy ? lastMixedPort : null,
        ),
      );
      _startTraffic();
    });
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    _phaseTimer?.cancel();
    final server = _rawTarget?.call(label);
    _emit(
      VpnStatus<S>(
        stage: VpnStage.connecting,
        server: server,
        detail: 'Importing profile',
      ),
    );
    _phaseTimer = Timer(const Duration(milliseconds: 1400), () {
      _emit(
        VpnStatus<S>(
          stage: VpnStage.connected,
          server: server,
          connectedSince: DateTime.now(),
          mode: lastMode,
          mixedPort: lastMode == TunnelMode.proxy ? lastMixedPort : null,
          // Пин узла отражаем как активный прокси; иначе первый из мок-списка.
          activeProxy: (serverId != null && serverId.isNotEmpty)
              ? serverId
              : mockServers.first.id,
        ),
      );
      _startTraffic();
    });
  }

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async {
    // Мок ничего не парсит: сырые данные игнорируются, отдаётся фиксированный
    // набор узлов, чтобы UI generic-режима работал без ядра.
    await Future<void>.delayed(const Duration(milliseconds: 200));
    return const ImportResult(name: 'Mock subscription', servers: mockServers);
  }

  @override
  Future<List<ProbeResult>> probe({
    Duration timeout = const Duration(seconds: 5),
  }) async {
    await Future<void>.delayed(const Duration(milliseconds: 300));
    const latencies = <int>[42, 128, -1];
    return <ProbeResult>[
      for (var i = 0; i < mockServers.length; i++)
        ProbeResult(
          id: mockServers[i].id,
          name: mockServers[i].name,
          country: mockServers[i].country,
          latencyMs: latencies[i % latencies.length],
        ),
    ];
  }

  @override
  Future<void> setPolicy(CorePolicy policy) async {
    lastPolicy = policy;
  }

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    lastMode = mode;
    lastMixedPort = mixedPort;
  }

  // --- CSM/1, ABI v3 ----------------------------------------------------------
  //
  // Ключевая пара здесь НАСТОЯЩАЯ: скаляр фиксирован, открытые ключи и `dtp`
  // считаются из него, поэтому `deviceAgree` даёт корректный ECDH и код выше
  // можно проверять целиком. Подпись же имитируется и НИКОГДА не пройдёт у
  // оператора: ECDSA в этом пакете нет, а выдавать за подпись то, что ею не
  // является, без этой оговорки было бы ровно тем видом лжи, против которого
  // написан весь CSM/1.

  static final Uint8List _mockScalar = Uint8List.fromList(
    List<int>.generate(32, (i) => (i * 7 + 11) & 0xff),
  );

  /// Префикс DER `SubjectPublicKeyInfo` для открытого ключа P-256:
  /// SEQUENCE { SEQUENCE { id-ecPublicKey, prime256v1 }, BIT STRING }.
  static const List<int> _spkiPrefix = <int>[
    0x30,
    0x59,
    0x30,
    0x13,
    0x06,
    0x07,
    0x2a,
    0x86,
    0x48,
    0xce,
    0x3d,
    0x02,
    0x01,
    0x06,
    0x08,
    0x2a,
    0x86,
    0x48,
    0xce,
    0x3d,
    0x03,
    0x01,
    0x07,
    0x03,
    0x42,
    0x00,
  ];

  @override
  Future<CsmDeviceKey> deviceKeygen({bool requireHardware = true}) async {
    final pub = p256PublicKey(_mockScalar);
    if (pub == null) {
      throw StateError('мок: скаляр вне диапазона');
    }
    final point = pub.uncompressed;
    final spki = Uint8List.fromList(<int>[..._spkiPrefix, ...point]);
    final dtp = sha256(spki).sublist(0, 16);
    return CsmDeviceKey(
      signingSpki: spki,
      agreementPublicKey: point,
      deviceThumbprint: _hex(dtp),
      // Уровень 3 и только он: у мока хранилища нет, и он это говорит.
      hardwareTier: 3,
      agreementKeyGeneration: 1,
    );
  }

  @override
  Future<Uint8List> deviceSign(Uint8List message) async {
    // Имитация: 64 байта, выведенные из сообщения, чтобы форма совпадала с
    // `r || s`. Криптографической силы ноль, и панель это отвергнет.
    final a = sha256(message);
    final b = sha256(<int>[0x01, ...a]);
    return Uint8List.fromList(<int>[...a, ...b]);
  }

  @override
  Future<CsmAgreement> deviceAgree({
    required Uint8List peerPublicKey,
    int rkv = 0,
    Uint8List? kdfInfo,
  }) async {
    final peer = p256DecodeUncompressed(peerPublicKey);
    if (peer == null) {
      throw const FormatException('мок: точка не декодируется');
    }
    final shared = p256Ecdh(_mockScalar, peer);
    final own = p256PublicKey(_mockScalar);
    if (shared == null || own == null) {
      throw StateError('мок: согласование не удалось');
    }
    return CsmAgreement(shared: shared, ownPublicKey: own.uncompressed);
  }

  @override
  Future<String> csmRequestSettings({
    required Map<int, Object?> want,
    Map<int, String> sel = const <int, String>{},
    String accountJwt = '',
  }) async {
    lastSettingsWrite = <int, Object?>{...want};
    lastSettingsSel = <int, String>{...sel};
    // Мок сети не касается: он отвечает пустым снимком, а не выдуманной
    // директивой. Придуманная директива была бы состоянием, которого ни один
    // оператор не подписывал.
    return '{}';
  }

  @override
  Future<String> csmState() async => csmStateJson;

  @override
  Future<String> csmLadder() async => csmLadderJson;

  @override
  Future<String> routeReport() async => routeReportJson;

  @override
  Future<String> csmEnroll({
    String origin = '',
    String code = '',
    String linkPin = '',
    String blobB64 = '',
    String subscriptionDomain = '',
    String accountJwt = '',
  }) async {
    lastEnroll = <String, String>{
      'origin': origin,
      'code': code,
      'link_pin': linkPin,
      'blob_b64': blobB64,
      'subscription_domain': subscriptionDomain,
      'account_jwt': accountJwt,
    };
    // Мок НЕ выдумывает pid и отпечаток корня: закрепить профиль на
    // придуманном корне значило бы показать пользователю личность оператора,
    // которой никто не подписывал. Тест подставляет снимок сам.
    return csmEnrollJson;
  }

  @override
  Future<String> csmRefresh({int timeoutSec = 30}) async {
    refreshCalls++;
    return csmStateJson;
  }

  @override
  Future<void> csmSetLadder({
    List<int> order = const <int>[],
    Map<int, bool> enabled = const <int, bool>{},
    String? proxy,
  }) async {
    lastLadderOrder = List<int>.unmodifiable(order);
    lastLadderEnabled = Map<int, bool>.unmodifiable(enabled);
    if (proxy != null) {
      lastLadderProxy = proxy;
    }
  }

  @override
  Future<void> csmAnswerCatalogChange({required bool accept}) async {
    catalogAnswers.add(accept);
  }

  @override
  Future<void> csmSelectProfile(String profileKey) async {
    selectedCsmProfile = profileKey;
  }

  /// Что мок отдаёт на [csmEnroll]. Пусто по умолчанию.
  String csmEnrollJson = '';

  /// Последний запрос регистрации.
  Map<String, String> lastEnroll = const <String, String>{};

  /// Сколько раз звали [csmRefresh].
  int refreshCalls = 0;

  /// Последний порядок и набор переключателей, отданные ядру.
  List<int> lastLadderOrder = const <int>[];
  Map<int, bool> lastLadderEnabled = const <int, bool>{};
  String? lastLadderProxy;

  /// Ответы на карточки смены набора ресурсов, в порядке поступления.
  final List<bool> catalogAnswers = <bool>[];

  /// Профиль, чьё хранилище CSM выбрано последним.
  String selectedCsmProfile = '';

  /// Что мок отдаёт на [csmState]. Пустая строка по умолчанию: мок НЕ выдумывает
  /// каталог, потому что выдуманный каталог поднял бы карточку 7.7.1 о
  /// сужении, которого не было. Тест подставляет сюда свой снимок.
  String csmStateJson = '';

  /// Что мок отдаёт на [csmLadder]. Пусто по той же причине: попытка, которой
  /// не было, в истории INV-17 быть не должна.
  String csmLadderJson = '';

  /// Что мок отдаёт на [routeReport].
  ///
  /// По умолчанию это ЧЕСТНОЕ «подъёма не было» — ровно то, что отдаёт ядро до
  /// первого [connect]. Мок не выдумывает здорового отчёта намеренно: экран,
  /// собранный против выдуманного «блок рекламы работает», показал бы зелёную
  /// галочку и на настоящем ядре, где база GEOSITE недоступна, а это и есть та
  /// самая ложь, ради устранения которой мост появился.
  ///
  /// Тест подставляет сюда свой отчёт. Заготовки для типичных состояний —
  /// [routeReportRaisedAdblockUnknownGeosite] и
  /// [routeReportRaisedDroppedRuleSource].
  String routeReportJson = routeReportNotRaised;

  /// Отчёт до первого подъёма: `known:false` с причиной `not_raised`.
  static const String routeReportNotRaised =
      '{"known":false,"reason":"not_raised",'
      '"detail":"no tunnel has been raised by this core instance, so there is '
      'nothing applied to report on",'
      '"tunnel_up":false,"rules":null,'
      '"geosite":{"required":false,"state":"unknown","reason":"not_raised",'
      '"path":"","size_bytes":0},'
      '"relay":{"state":"not_requested","dialer_proxy_seen":false}}';

  /// Заготовка: пресет «Только блок рекламы» поднят, а состояние базы GEOSITE
  /// неизвестно. Это САМЫЙ частый реальный случай — доверенного каталога нет,
  /// `geox-url` не пишется, и обещать пользователю работающий блок рекламы
  /// нечем.
  static const String routeReportRaisedAdblockUnknownGeosite =
      '{"known":true,"raised_at_ms":1756800000000,"tunnel_up":true,'
      '"source":"preset",'
      '"preset":{"preset_id":"adblock","preset_name":"Только блок рекламы",'
      '"emoji":"🛡️","final_action":"DIRECT","rules":2,"dropped_rules":0,'
      '"rules_by_type":{"GEOIP":1,"GEOSITE":1},'
      '"geosite_tags":["category-ads-all"]},'
      '"rules":2,'
      '"geosite":{"required":true,"tags":["category-ads-all"],'
      '"state":"unknown","reason":"geox_unmanaged",'
      '"detail":"no trusted catalog and no local GeoSite.dat",'
      '"path":"/data/caramba/GeoSite.dat","size_bytes":0},'
      '"relay":{"state":"not_requested","dialer_proxy_seen":false}}';

  /// Заготовка: пресет «Россия (умный)» поднят, но оба его внешних списка не
  /// доехали. Пресет включён и наполовину пуст — экран обязан это показать.
  static const String routeReportRaisedDroppedRuleSource =
      '{"known":true,"raised_at_ms":1756800000000,"tunnel_up":true,'
      '"source":"preset",'
      '"preset":{"preset_id":"ru-smart","preset_name":"Россия (умный)",'
      '"emoji":"🇷🇺","country":"RU","final_action":"DIRECT",'
      '"rules":10,"dropped_rules":2,'
      '"rules_by_type":{"GEOIP":2,"GEOSITE":8},'
      '"geosite_tags":["telegram","instagram","facebook","twitter","youtube",'
      '"discord","openai","category-ru"],'
      '"sources":['
      '{"name":"ru-blocked","state":"dropped","reason":"no_mirror",'
      '"detail":"no verified file and no mirror base URL","rules":1,'
      '"kept_rules":0},'
      '{"name":"ru-blocked-ip","state":"dropped","reason":"no_mirror",'
      '"detail":"no verified file and no mirror base URL","rules":1,'
      '"kept_rules":0}]},'
      '"rules":10,'
      '"geosite":{"required":true,"tags":["telegram"],"state":"verified",'
      '"path":"/data/caramba/GeoSite.dat","size_bytes":1048576},'
      '"relay":{"requested":"TR","state":"sent","dialer_proxy_seen":false,'
      '"detail":"relay_country was sent to the panel, but the applied body '
      'carries no dialer-proxy key"}}';

  /// Последняя карта `want`, переданная в [csmRequestSettings].
  Map<int, Object?> lastSettingsWrite = const <int, Object?>{};

  /// Последняя карта `sel`, переданная в [csmRequestSettings].
  Map<int, String> lastSettingsSel = const <int, String>{};

  static String _hex(List<int> bytes) {
    final sb = StringBuffer();
    for (final b in bytes) {
      sb.write(b.toRadixString(16).padLeft(2, '0'));
    }
    return sb.toString();
  }

  @override
  Future<void> disconnect() async {
    _phaseTimer?.cancel();
    _stopTraffic();
    _emit(VpnStatus<S>(stage: VpnStage.disconnected, server: _last.server));
  }

  /// У мока нет платформы, которую можно переспросить: он САМ и есть источник
  /// состояния, и его собственный снимок всегда актуален.
  @override
  Future<VpnStatus<S>> refreshStatus() async => _last;

  void _startTraffic() {
    _stopTraffic();
    _trafficTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      _seed++;
      // Псевдослучайный, но плавный профиль (без dart:math import — детерминирован).
      final wobble = (_seed * 2654435761) & 0x7fffffff;
      final down = 4 * 1024 * 1024 + (wobble % (10 * 1024 * 1024));
      final up = 256 * 1024 + (wobble % (1024 * 1024));
      _trafficCtrl.add(
        TrafficStats(
          downBps: down,
          upBps: up,
          downTotal: down * _seed,
          upTotal: up * _seed,
        ),
      );
    });
  }

  void _stopTraffic() {
    _trafficTimer?.cancel();
    _trafficTimer = null;
    _trafficCtrl.add(TrafficStats.zero);
  }

  @override
  Future<void> dispose() async {
    _phaseTimer?.cancel();
    _trafficTimer?.cancel();
    await _statusCtrl.close();
    await _trafficCtrl.close();
  }
}

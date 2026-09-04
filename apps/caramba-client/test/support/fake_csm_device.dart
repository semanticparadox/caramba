/// Общая половина CSM/1 для тестовых двойников [VpnConnection].
///
/// Ключи устройства и запись настроек живут в ядре, а тесты приложения смотрят
/// на слой выше. Миксин даёт им честные заглушки, чтобы каждый двойник не
/// переписывал одно и то же, и заодно ЗАПОМИНАЕТ отправленную карту `want`:
/// именно она и есть то, что проверяют тесты пути записи.
library;

import 'dart:typed_data';

import 'package:caramba_vpn/caramba_vpn.dart';

mixin FakeCsmDevice {
  /// Карты `want`, ушедшие через [csmRequestSettings], в порядке вызовов.
  final List<Map<int, Object?>> sentWrites = <Map<int, Object?>>[];

  /// Когда непусто, [csmRequestSettings] бросает с этим текстом: так тест
  /// проверяет, что отказ сети не теряет очередь.
  String failWritesWith = '';

  /// Что вернёт [deviceKeygen]. Уровень 3 по умолчанию: у теста хранилища нет,
  /// и притворяться Secure Enclave он не будет.
  int hardwareTier = 3;

  Future<CsmDeviceKey> deviceKeygen({bool requireHardware = true}) async {
    final point = Uint8List(65)..[0] = 0x04;
    return CsmDeviceKey(
      signingSpki: Uint8List(91),
      agreementPublicKey: point,
      deviceThumbprint: '4f0f22569564aab09a2d1a75c132d955',
      hardwareTier: hardwareTier,
      agreementKeyGeneration: 1,
    );
  }

  Future<Uint8List> deviceSign(Uint8List message) async => Uint8List(64);

  Future<CsmAgreement> deviceAgree({
    required Uint8List peerPublicKey,
    int rkv = 0,
    Uint8List? kdfInfo,
  }) async => CsmAgreement(
    shared: Uint8List(32),
    ownPublicKey: Uint8List(65)..[0] = 0x04,
  );

  Future<String> csmRequestSettings({
    required Map<int, Object?> want,
    Map<int, String> sel = const <int, String>{},
    String accountJwt = '',
  }) async {
    if (failWritesWith.isNotEmpty) {
      throw StateError(failWritesWith);
    }
    sentWrites.add(Map<int, Object?>.from(want));
    return '{}';
  }

  /// Что фейк отдаёт на [csmState]. Пустая строка по умолчанию: выдуманный
  /// каталог поднял бы карточку 02-SPEC.md 7.7.1 о сужении, которого не было.
  String csmStateJson = '';

  /// Что фейк отдаёт на [csmLadder]. Пусто по той же причине: попытки, которой
  /// не было, в истории INV-17 быть не должно.
  String csmLadderJson = '';

  Future<String> csmState() async => csmStateJson;

  Future<String> csmLadder() async => csmLadderJson;

  /// Что фейк отдаёт на [routeReport]. По умолчанию — честное «подъёма не
  /// было»: выдуманный здоровый отчёт научил бы экран рисовать работающий блок
  /// рекламы там, где база GEOSITE недоступна, а это ровно та ложь, ради
  /// устранения которой мост и появился.
  String routeReportJson = MockVpnConnection.routeReportNotRaised;

  Future<String> routeReport() async => routeReportJson;

  /// Что фейк отдаёт на [csmEnroll]. Пусто по умолчанию: выдуманный pid и
  /// отпечаток корня закрепили бы профиль на личности, которой никто не
  /// подписывал.
  String csmEnrollJson = '';

  /// Запросы регистрации, в порядке вызовов.
  final List<Map<String, String>> enrollCalls = <Map<String, String>>[];

  /// Сколько раз звали [csmRefresh].
  int refreshCalls = 0;

  /// Последний порядок и набор переключателей, отданные ядру.
  List<int> lastLadderOrder = const <int>[];
  Map<int, bool> lastLadderEnabled = const <int, bool>{};

  /// Ответы на карточки смены набора ресурсов, в порядке поступления.
  final List<bool> catalogAnswers = <bool>[];

  /// Профиль, чьё хранилище CSM выбрано последним.
  String selectedCsmProfile = '';

  /// Когда непусто, [csmAnswerCatalogChange] бросает с этим текстом: так тест
  /// проверяет, что отказ ядра НЕ снимает карточку.
  String failCatalogAnswerWith = '';

  Future<String> csmEnroll({
    String origin = '',
    String code = '',
    String linkPin = '',
    String blobB64 = '',
    String subscriptionDomain = '',
    String accountJwt = '',
  }) async {
    enrollCalls.add(<String, String>{
      'origin': origin,
      'code': code,
      'link_pin': linkPin,
      'blob_b64': blobB64,
      'subscription_domain': subscriptionDomain,
      'account_jwt': accountJwt,
    });
    return csmEnrollJson;
  }

  Future<String> csmRefresh({int timeoutSec = 30}) async {
    refreshCalls++;
    return csmStateJson;
  }

  Future<void> csmSetLadder({
    List<int> order = const <int>[],
    Map<int, bool> enabled = const <int, bool>{},
    String? proxy,
  }) async {
    lastLadderOrder = List<int>.unmodifiable(order);
    lastLadderEnabled = Map<int, bool>.unmodifiable(enabled);
  }

  Future<void> csmAnswerCatalogChange({required bool accept}) async {
    if (failCatalogAnswerWith.isNotEmpty) {
      throw StateError(failCatalogAnswerWith);
    }
    catalogAnswers.add(accept);
  }

  Future<void> csmSelectProfile(String profileKey) async {
    selectedCsmProfile = profileKey;
  }
}

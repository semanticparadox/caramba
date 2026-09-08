/// Мок обязан отдавать РАЗБИРАЕМЫЙ отчёт той же формы, что и ядро.
///
/// Экраны собираются против мока, а живут против Go. Заготовка, которая не
/// разбирается или называет поля иначе, чем `api.RouteReport`, даёт экран,
/// который проходит тесты и врёт на устройстве — ровно то, из-за чего владелец
/// и сказал про блок рекламы «непонятно, работают или нет».
library;

import 'dart:convert';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

Map<String, Object?> decode(String raw) =>
    jsonDecode(raw) as Map<String, Object?>;

void main() {
  test('по умолчанию мок честно говорит, что подъёма не было', () async {
    final mock = MockVpnConnection<Object>();
    final rep = decode(await mock.routeReport());
    expect(rep['known'], isFalse);
    expect(rep['reason'], 'not_raised');
    // null, а не 0: «правил ноль» и «сколько правил — неизвестно» это разные
    // ответы, и экран обязан их различать.
    expect(rep['rules'], isNull);
    expect((rep['geosite']! as Map)['state'], 'unknown');
    expect((rep['relay']! as Map)['state'], 'not_requested');
  });

  test('заготовка adblock не обещает работающую базу GEOSITE', () {
    final rep =
        decode(MockVpnConnection.routeReportRaisedAdblockUnknownGeosite);
    expect(rep['known'], isTrue);
    expect(rep['source'], 'preset');

    final geosite = rep['geosite']! as Map<String, Object?>;
    expect(geosite['required'], isTrue);
    expect(geosite['tags'], <String>['category-ads-all']);
    // Доверенного каталога нет: geox-url не пишется, и обещать пользователю
    // работающий блок рекламы нечем.
    expect(geosite['state'], 'unknown');
    expect(geosite['reason'], 'geox_unmanaged');
    expect(geosite['path'], isNotEmpty);
  });

  test('заготовка ru-smart называет каждый не доехавший список', () {
    final rep = decode(MockVpnConnection.routeReportRaisedDroppedRuleSource);
    final preset = rep['preset']! as Map<String, Object?>;
    expect(preset['preset_id'], 'ru-smart');
    expect(preset['dropped_rules'], greaterThan(0));

    final sources = preset['sources']! as List<Object?>;
    expect(sources, hasLength(2));
    for (final entry in sources) {
      final src = entry! as Map<String, Object?>;
      expect(src['state'], 'dropped');
      expect(src['reason'], 'no_mirror');
      expect(src['kept_rules'], 0);
      expect(src['rules'], greaterThan(0));
    }

    // Страна входа ушла в панель, но цепочки в применённом теле нет: «отправили»
    // и «работает» здесь разные утверждения, и второго ядро не делает.
    final relay = rep['relay']! as Map<String, Object?>;
    expect(relay['state'], 'sent');
    expect(relay['dialer_proxy_seen'], isFalse);
  });

  test('подставленный отчёт доезжает до вызывающего без изменений', () async {
    final mock = MockVpnConnection<Object>()
      ..routeReportJson =
          MockVpnConnection.routeReportRaisedAdblockUnknownGeosite;
    expect(
      await mock.routeReport(),
      MockVpnConnection.routeReportRaisedAdblockUnknownGeosite,
    );
  });
}

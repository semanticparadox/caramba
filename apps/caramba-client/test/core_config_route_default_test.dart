import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/state/core_config_state.dart';

/// Маршрут по умолчанию обязан следовать за ПОЛЬЗОВАТЕЛЕМ, а не за константой.
///
/// Жалоба владельца: у всех, включая его самого в США, стоял `ru-smart`. Этот
/// пресет заканчивается действием DIRECT, поэтому вне России туннель поднимался
/// и почти ничего в себя не пускал.
void main() {
  group('defaultRouteIndexForCountry', () {
    test('kGlobalRouteIndex указывает ровно на пресет global', () {
      // Число продублировано в core_config_state.dart намеренно (там же
      // объяснено почему). Разъедется — упадёт здесь, а не у пользователя.
      expect(kLegacyRouteIndexByCoreId['global'], kGlobalRouteIndex);
      expect(RoutingMode.defaults[kGlobalRouteIndex].id, 'global');
    });

    test('страна с национальным пресетом получает свой', () {
      expect(defaultRouteIndexForCountry('RU'), 0); // ru-smart
      expect(defaultRouteIndexForCountry('ru'), 0);
      expect(
        RoutingMode.defaults[defaultRouteIndexForCountry('IR')].id,
        'ir-smart',
      );
      expect(
        RoutingMode.defaults[defaultRouteIndexForCountry('BY')].id,
        'by-smart',
      );
      expect(
        RoutingMode.defaults[defaultRouteIndexForCountry('CN')].id,
        'cn-smart',
      );
    });

    test('страна без национального пресета получает global, а не чужой', () {
      for (final cc in <String>['US', 'DE', 'GB', 'PL', 'KZ']) {
        expect(
          defaultRouteIndexForCountry(cc),
          kGlobalRouteIndex,
          reason: '$cc не должен получать национальный режим другой страны',
        );
      }
    });

    test('неизвестная страна не угадывается', () {
      expect(defaultRouteIndexForCountry(null), kGlobalRouteIndex);
      expect(defaultRouteIndexForCountry(''), kGlobalRouteIndex);
      expect(defaultRouteIndexForCountry('  '), kGlobalRouteIndex);
      expect(defaultRouteIndexForCountry('USA'), kGlobalRouteIndex);
    });

    test('у RU первым в реестре идёт ru-smart, а не ru-full', () {
      // В реестре два пресета со страной RU. Умолчанием обязан быть умный:
      // полный обход через VPN ломает банки и госуслуги.
      final ruPresets = kCoreRoutePresets
          .where((p) => p.countryCode == 'RU')
          .toList();
      expect(ruPresets.length, greaterThanOrEqualTo(2));
      expect(ruPresets.first.id, 'ru-smart');
    });
  });

  group('CoreConfig: выбор человека против умолчания', () {
    test('свежая установка стартует на global', () {
      expect(const CoreConfig().route, kGlobalRouteIndex);
      expect(const CoreConfig().routeChosen, isFalse);
    });

    test('старая запись не меняет маршрут сама по себе', () {
      // Ключа route_chosen нет — запись сделана прежней версией. Индекс 0
      // неразличим («выбрал ru-smart» против «не открывал пикер»), и апгрейд
      // обязан оставить его на месте до появления страны.
      final legacy = CoreConfig.fromJson(<String, dynamic>{'route': 0});
      expect(legacy.route, 0);
      expect(legacy.routeChosen, isFalse);
    });

    test('ненулевой индекс старой записи — доказанный выбор человека', () {
      // При старой схеме умолчанием был 0, поэтому 3 мог появиться только из
      // пикера.
      final legacy = CoreConfig.fromJson(<String, dynamic>{'route': 3});
      expect(legacy.route, 3);
      expect(legacy.routeChosen, isTrue);
    });

    test('route_chosen переживает сериализацию', () {
      final chosen = const CoreConfig().copyWith(route: 2, routeChosen: true);
      final back = CoreConfig.fromJson(chosen.toJson());
      expect(back.route, 2);
      expect(back.routeChosen, isTrue);
    });
  });

  group('CoreConfigNotifier.adoptUserCountry', () {
    test('американец уходит с ru-smart, россиянин на нём остаётся', () {
      final us = CoreConfigNotifier()
        ..hydrate(CoreConfig.fromJson(<String, dynamic>{'route': 0}));
      us.adoptUserCountry('US');
      expect(us.state.route, kGlobalRouteIndex);

      final ru = CoreConfigNotifier()
        ..hydrate(CoreConfig.fromJson(<String, dynamic>{'route': 0}));
      ru.adoptUserCountry('RU');
      expect(ru.state.route, 0);
    });

    test('явный выбор человека сильнее страны', () {
      final n = CoreConfigNotifier()..hydrate(const CoreConfig());
      n.setRoute(0); // человек сам выбрал ru-smart, находясь в США
      n.adoptUserCountry('US');
      expect(n.state.route, 0, reason: 'выбранный маршрут трогать нельзя');
      expect(n.state.routeChosen, isTrue);
    });

    test('неизвестная страна ничего не двигает', () {
      final n = CoreConfigNotifier()
        ..hydrate(CoreConfig.fromJson(<String, dynamic>{'route': 0}));
      n.adoptUserCountry(null);
      n.adoptUserCountry('');
      n.adoptUserCountry('USA');
      expect(n.state.route, 0);
    });

    test('умолчание можно поправить второй раз: страна ещё не выбор', () {
      final n = CoreConfigNotifier()..hydrate(const CoreConfig());
      n.adoptUserCountry('RU');
      expect(n.state.route, 0);
      expect(n.state.routeChosen, isFalse);
      // Человек переехал / GeoIP наконец ответил точнее.
      n.adoptUserCountry('DE');
      expect(n.state.route, kGlobalRouteIndex);
    });
  });
}

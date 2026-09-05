// Как называется машина в списке серверов.
//
// Экран стал плоским, и заголовок строки — единственное, по чему человек
// отличает одну машину от другой. Три источника имени дают три разных ответа,
// и каждый из них здесь зафиксирован: пока правила жили только в UI, панельный
// узел с одним инбаундом успел назваться тегом этого инбаунда.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/features/servers/fleet_alignment.dart';

const _ok = Availability.available(Provenance.nothing);

InboundOffer _inbound(String tag) => InboundOffer(
  key: const ProtocolKey(protocol: 'vless', transport: 'tcp', security: 'tls'),
  tag: tag,
  label: tag,
  proxyName: tag,
  availability: _ok,
);

ExitOffer _exit({
  int? panelNodeId,
  String label = '',
  String key = 'a.example',
  String countryName = 'Германия',
  List<String> tags = const <String>['in-1'],
}) => ExitOffer(
  key: key,
  panelNodeId: panelNodeId,
  countryCode: 'DE',
  countryName: countryName,
  label: label,
  inbounds: tags.map(_inbound).toList(),
  inboundsKnown: _ok,
  availability: _ok,
);

void main() {
  group('имя машины', () {
    test('у панели побеждает имя, которое дал оператор', () {
      // Здесь и была ошибка: правило про единственный инбаунд стояло раньше, и
      // узел назывался «only-in» вместо имени оператора.
      expect(
        machineTitleOf(
          _exit(panelNodeId: 7, label: 'Node #7', tags: const ['only-in']),
        ),
        'Node #7',
      );
    });

    test('в импорте единственный прокси и есть имя машины', () {
      // Имени у машины нет, но имя её единственного прокси относится именно к
      // ней — потерять его значило бы отдать человеку меньше, чем сказал
      // источник.
      expect(machineTitleOf(_exit(tags: const ['Amsterdam #1'])), 'Amsterdam #1');
    });

    test('когда прокси много и имени нет, машину называет страна', () {
      // Общего имени у восьми входов нет, а ключ машины — её АДРЕС. Адрес не
      // имя, и заголовком он быть не должен.
      final title = machineTitleOf(
        _exit(key: '85.215.196.151', tags: const ['a', 'b', 'c']),
      );
      expect(title, 'Германия');
      expect(title, isNot(contains('85.215')));
    });

    test('адрес в label именем не считается', () {
      // Сборщик импортированного тела кладёт в `label` тот же host, что и в
      // ключ: своего имени у машины нет. Первая версия правила брала `label`
      // как имя, и на экране остался «85.215.196.151».
      expect(
        machineTitleOf(
          _exit(
            key: '85.215.196.151',
            label: '85.215.196.151',
            tags: const ['a', 'b'],
          ),
        ),
        'Германия',
      );
    });

    test('тёзки получают номер, одиночка остаётся без него', () {
      expect(
        disambiguateTitles(const ['Германия', 'Канада', 'Германия']),
        const ['Германия #1', 'Канада', 'Германия #2'],
      );
    });
  });
}

// Строки «Типа подключения» — ВХОДЫ, а не семейства протоколов.
//
// Владелец спросил про инбаунды, а экран показывал четыре строки на восемь
// входов немецкой машины: VLESS, Hysteria2, TUIC, NaiveProxy. За словом «VLESS»
// стояли пять разных транспортов (reality, grpc, ws, httpupgrade, tcp+tls), и
// строка показывала ЛУЧШЕЕ из них. Из-за этого сломанный httpupgrade был
// невидим: его отказ растворялся в числе соседа по семейству.
//
// Фикстура снята с живой подписки 34 (`?client=sing-box`, 2026-09-06): немецкий
// узел 85.215.196.151 отдаёт восемь входов, и панельный генератор выпускает
// каждый из них дважды — прямым набором и через вход («via 🇷🇺»), — итого
// шестнадцать прокси. Числа портов и имена прокси взяты из тела как есть.
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_builder.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart'
    show autoRowTexts, optionIndexForProtocol, protocolRowTitle;
import 'package:caramba_client/vpn/vpn_models.dart' show ImportedServer;

const _de = '85.215.196.151';
const _ca = '158.69.213.88';

/// Один вход немецкой машины в двух экземплярах: прямой набор и «via 🇷🇺».
/// Ровно так его отдаёт панель, и ровно так его разбирает ядро.
List<ImportedServer> _both({
  required String name,
  required String type,
  required int port,
  String transport = '',
  String security = 'tls',
  String server = _de,
  String country = 'DE',
}) => <ImportedServer>[
  ImportedServer(
    id: name,
    name: name,
    type: type,
    server: server,
    port: port,
    country: country,
    transport: transport,
    security: security,
  ),
  ImportedServer(
    id: '$name via 🇷🇺',
    name: '$name via 🇷🇺',
    type: type,
    server: server,
    port: port,
    country: country,
    transport: transport,
    security: security,
  ),
];

/// Живой немецкий узел: восемь входов, шестнадцать прокси.
final List<ImportedServer> _germany = <ImportedServer>[
  ..._both(name: '🇩🇪 Stream', type: 'vless', port: 10400, transport: 'grpc'),
  ..._both(name: '🇩🇪 WebSocket', type: 'vless', port: 12400, transport: 'ws'),
  ..._both(
    name: '🇩🇪 Stealth',
    type: 'vless',
    port: 443,
    transport: 'tcp',
    security: 'reality',
  ),
  ..._both(name: '🇩🇪 Secure', type: 'vless', port: 14400, transport: 'tcp'),
  ..._both(
    name: '🇩🇪 HTTP',
    type: 'vless',
    port: 13400,
    transport: 'httpupgrade',
  ),
  // У QUIC-семейств транспорта поверх TCP нет вовсе, и пустая строка честнее
  // выдуманного «tcp».
  ..._both(name: '🇩🇪 TUIC', type: 'tuic', port: 16400),
  ..._both(name: '🇩🇪 Speed', type: 'hysteria2', port: 11466),
  ..._both(name: '🇩🇪 Naive', type: 'naive', port: 15400, transport: 'tcp'),
];

final List<ImportedServer> _fleet = <ImportedServer>[
  ..._germany,
  ..._both(
    name: '🇨🇦 Stealth',
    type: 'vless',
    port: 8443,
    transport: 'tcp',
    security: 'reality',
    server: _ca,
    country: 'CA',
  ),
  ..._both(
    name: '🇨🇦 HTTP',
    type: 'vless',
    port: 13400,
    transport: 'httpupgrade',
    server: _ca,
    country: 'CA',
  ),
];

/// То же тело глазами ядра, которое про форму молчит (сборка старше полей
/// `transport`/`security`).
List<ImportedServer> get _formless => <ImportedServer>[
  for (final s in _germany)
    ImportedServer(
      id: s.id,
      name: s.name,
      type: s.type,
      server: s.server,
      port: s.port,
      country: s.country,
    ),
];

ProtocolSlate _slateOfGermany(List<ImportedServer> servers) =>
    protocolSlateOf(buildImportedOffering(servers: servers), exitKey: _de);

void main() {
  group('импортированное тело: строка это вход, а не семейство', () {
    test('восемь входов немецкой машины — восемь строк, а не четыре', () {
      final slate = _slateOfGermany(_germany);

      expect(slate.scope, ProtocolScope.singleExit);
      expect(slate.rows.length, 8);
      expect(
        slate.rows.map(protocolRowTitleOf),
        containsAll(<String>[
          'VLESS · grpc · tls',
          'VLESS · ws · tls',
          'VLESS · tcp · reality',
          'VLESS · tcp · tls',
          'VLESS · httpupgrade · tls',
          'TUIC · tls',
          'Hysteria2 · tls',
          'NaiveProxy · tcp · tls',
        ]),
      );
      // Ровно то, чего не было: пять транспортов VLESS больше не одна строка.
      expect(slate.rows.where((r) => r.key.protocol == 'vless').length, 5);
    });

    test('строка не смешивает транспорты — сломанному негде спрятаться', () {
      // Это и есть весь дефект в одном утверждении. Пока httpupgrade жил в
      // строке «VLESS» вместе с reality, число строки приходило от reality
      // (474 мс), а отказ httpupgrade не показывался нигде.
      final offering = buildImportedOffering(servers: _germany);
      final byName = <String, ImportedServer>{
        for (final s in _germany) s.id: s,
      };

      for (final row in protocolSlateOf(offering, exitKey: _de).rows) {
        for (final proxy in row.proxyNames) {
          final s = byName[proxy]!;
          expect(s.transport, row.key.transport, reason: proxy);
          expect(s.security, row.key.security, reason: proxy);
          expect(s.type, row.key.protocol, reason: proxy);
        }
      }
    });

    test('«via 🇷🇺» не удваивает список, но из строки не пропадает', () {
      // Оба прокси упираются в ОДИН вход одной машины (13400,
      // vless/httpupgrade/tls) и различаются только путём набора. Путь — вопрос
      // контрола «Relay (вход)»; две строки с побуквенно одинаковым заголовком
      // и разными числами были бы новой загадкой вместо решённой.
      final row = _slateOfGermany(
        _germany,
      ).rows.singleWhere((r) => r.key.transport == 'httpupgrade');

      expect(row.exitKeys, <String>[_de]);
      expect(row.proxyNames, <String>['🇩🇪 HTTP', '🇩🇪 HTTP via 🇷🇺']);
    });

    test('область «весь флот» считает узлы, а не прокси', () {
      // Без выбранного узла строка собирает машины: httpupgrade предлагают обе,
      // и прокси у неё четыре — по два на машину.
      final slate = protocolSlateOf(buildImportedOffering(servers: _fleet));

      expect(slate.scope, ProtocolScope.wholeFleet);
      final row = slate.rows.singleWhere(
        (r) => r.key.transport == 'httpupgrade',
      );
      expect(row.exitKeys, unorderedEquals(<String>[_de, _ca]));
      expect(row.proxyNames.length, 4);
    });

    test('форма разводит строки и по опциям ядра, а не только по виду', () {
      final rows = _slateOfGermany(_germany).rows;
      const options = ProtocolOption.defaults;

      final reality = rows.singleWhere((r) => r.key.security == 'reality');
      final ws = rows.singleWhere((r) => r.key.transport == 'ws');
      // `VLESS-Reality` уточняет форму, и теперь ей есть по чему сработать:
      // пока security был пуст, в неё не попадал никто.
      expect(
        options[optionIndexForProtocol(reality.key, options)!].id,
        'VLESS-Reality',
      );
      expect(options[optionIndexForProtocol(ws.key, options)!].id, 'VLESS');
      // Naive ядру попросить нечем — строка остаётся, но не выбирается.
      final naive = rows.singleWhere((r) => r.key.protocol == 'naive');
      expect(optionIndexForProtocol(naive.key, options), isNull);
    });

    test('форму назвали — закрепление формы подтверждаемо', () {
      final caps = buildImportedOffering(servers: _germany).capabilities;
      expect(caps.protocolPin.isAvailable, isTrue);
      expect(caps.protocolPin.isVerifiable, isTrue);
    });
  });

  group('ядро про форму молчит — список честно схлопывается обратно', () {
    test('строк снова четыре, и причина названа вслух', () {
      final slate = _slateOfGermany(_formless);

      expect(slate.rows.length, 4);
      expect(slate.known.status, OfferingStatus.unknown);
      expect(slate.known.reason, OfferingReason.sourceDoesNotReportTransport);
      for (final row in slate.rows) {
        expect(row.key.isFullyQualified, isFalse);
      }
    });

    test('подтвердить форму нечем, и это сказано причиной', () {
      final caps = buildImportedOffering(servers: _formless).capabilities;
      // Закрепить прокси всё ещё можно: имя в теле есть.
      expect(caps.protocolPin.isAvailable, isTrue);
      expect(caps.protocolPin.isVerifiable, isFalse);
      expect(
        caps.protocolPin.verification.reason,
        OfferingReason.sourceDoesNotReportTransport,
      );
    });
  });

  group('строка «Авто»: словарь общий с Главной, точка одна', () {
    const auto = ProtocolOption(
      id: '',
      name: 'Авто',
      desc: 'Приложение само выбирает протокол.',
      icon: '',
      auto: true,
    );

    test('расхождение с держателем — «не в силе», а не «устарело»', () {
      // Экран говорил «устарело» на все причины сразу и посылал перезамерять
      // там, где перезамер не изменит ничего: не в силе сам ВЫБОР.
      final t = autoRowTexts(
        option: auto,
        label: const AutoLabel(
          choice: 'VLESS · tcp · reality',
          source: 'по замеру 12 мин назад',
          stale: AutoStaleReason.pinDisagrees,
        ),
        choice: 'VLESS · tcp · reality',
      );
      expect(t.badge, 'не в силе');
      expect(t.title, 'Авто · VLESS · tcp · reality');
    });

    test('устаревший замер — «устарело»', () {
      final t = autoRowTexts(
        option: auto,
        label: const AutoLabel(
          choice: 'Hysteria2',
          source: 'по замеру 7 ч назад',
          stale: AutoStaleReason.age,
        ),
        choice: 'Hysteria2',
      );
      expect(t.badge, 'устарело');
    });

    test('слово берётся у AutoLabel во всех ветках сразу', () {
      for (final reason in AutoStaleReason.values) {
        final label = AutoLabel(
          choice: 'VLESS',
          source: 'сейчас в туннеле',
          stale: reason,
        );
        expect(
          autoRowTexts(option: auto, label: label, choice: 'VLESS').badge,
          label.badge,
          reason: reason.name,
        );
      }
    });

    test('точка в подписи ровно одна', () {
      // Причина устаревания приезжает уже законченным предложением, и слепо
      // дописанная точка давала «переподключении..».
      for (final reason in AutoStaleReason.values) {
        final t = autoRowTexts(
          option: auto,
          label: AutoLabel(
            choice: 'VLESS',
            source: 'сейчас в туннеле',
            stale: reason,
          ),
          choice: 'VLESS',
        );
        expect(t.subtitle, isNot(contains('..')), reason: reason.name);
        expect(t.subtitle, endsWith('.'), reason: reason.name);
      }
      // Выбора нет — подпись приходит без точки, и её дописывают.
      expect(
        autoRowTexts(
          option: auto,
          label: const AutoLabel(),
          choice: '',
        ).subtitle,
        endsWith('Выберется при подключении.'),
      );
    });
  });
}

/// Заголовок строки так, как его увидит человек.
String protocolRowTitleOf(ProtocolRow row) => protocolRowTitle(row.key);

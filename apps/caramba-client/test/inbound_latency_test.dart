// Задержка ОДНОГО инбаунда и вердикт его проверки — без виджетов.
//
// Вывод легко сломать молча. Задержка строки собирается из НЕСКОЛЬКИХ прокси,
// и любая арифметика тут даёт число, которое выглядит правдоподобно: среднее,
// худшее, первое попавшееся. А вердикт — та самая граница, ради которой ядро и
// перестало подменять провал URL-теста временем TCP: «адрес отвечает» и «вход
// пропустил запрос» выглядят одинаковым числом и означают разное.
//
// Поэтому проверяется не «работает ли», а границы: «не мерили» ≠ «отказ»,
// «мерить нечего» ≠ «не мерили», лучший из ответивших ≠ лучший из названных,
// и число без подтверждения ≠ число.

import 'package:caramba_vpn/caramba_vpn.dart' show ProbeVerdict;
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/features/protocol/inbound_latency.dart';

ProtocolRow _row(
  ProtocolKey key,
  List<String> proxyNames, {
  List<String>? exitKeys,
}) => ProtocolRow(
  key: key,
  label: '',
  exitKeys: exitKeys ?? <String>['node-1'],
  proxyNames: proxyNames,
  availability: const Availability.available(
    Provenance(OfferingSource.panelRest, '/app/servers[].inbounds[]'),
  ),
);

const _reality = ProtocolKey(
  protocol: 'vless',
  transport: 'tcp',
  security: 'reality',
);
const _hy2 = ProtocolKey(
  protocol: 'hysteria2',
  transport: 'udp',
  security: 'tls',
);

/// Замер как его кладёт на профиль [ProbeSnapshot.fromResults]: число, вердикт
/// и TCP приходят одной записью и одним моментом времени.
InboundLatencyLookup _lookup(
  Map<String, (int, ProbeVerdict)> results, {
  bool measuring = false,
  Map<String, int> tcp = const <String, int>{},
}) => InboundLatencyLookup(
  snapshot: results.isEmpty && tcp.isEmpty
      ? null
      : ProbeSnapshot(
          latencyMs: <String, int>{
            for (final e in results.entries) e.key: e.value.$1,
          },
          verdicts: <String, String>{
            for (final e in results.entries)
              if (e.value.$2 != ProbeVerdict.unknown) e.key: e.value.$2.wire,
          },
          tcpMs: tcp,
          updatedMs: DateTime(2026, 9, 5, 12).millisecondsSinceEpoch,
        ),
  measuring: measuring,
);

void main() {
  group('задержка инбаунда', () {
    test('число собственного замера, а не операторское', () {
      // Панель отдаёт одно `latency_ms` на всю машину — расстояние УЗЛА до его
      // цели. У инбаунда своего числа у неё нет вовсе, поэтому источником
      // здесь может быть только клиент.
      final l = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (180, ProbeVerdict.ok),
      }).of(_row(_reality, <String>['DE Stealth']));
      expect(l.latency.ms, 180);
      expect(l.latency.source, LatencySource.client);
      // Сквозь вход прошёл настоящий запрос: число говорит само, добавлять
      // нечего.
      expect(l.verdictNote, isNull);
      expect(l.isUnconfirmed, isFalse);
    });

    test('«не мерили» и «отказ» — разные ответы', () {
      final row = _row(_reality, <String>['DE Stealth']);

      final never = _lookup(const <String, (int, ProbeVerdict)>{}).of(row);
      expect(never.latency.source, LatencySource.none);
      expect(never.latency.isTimeout, isFalse);
      expect(never.verdictNote, isNull);

      // Замер прокси назвал, и вход запрос не пропустил. Склеить это с «не
      // мерили» значило бы спрятать мёртвый вход среди неизмеренных — и в
      // цвете, и в сортировке.
      final dead = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (-1, ProbeVerdict.authRejected),
      }).of(row);
      expect(dead.latency.source, LatencySource.client);
      expect(dead.latency.isTimeout, isTrue);
      expect(dead.verdictNote, contains('не принял ключ подписки'));
    });

    test(
      'число без подтверждения названо словами, а не выдано за задержку',
      () {
        // Сборка без ядра меряет только TCP и говорит об этом вердиктом
        // `tcp_only`. Число настоящее — вопрос слабее, и строка обязана это
        // произнести: рядом стоят числа, добытые настоящим запросом.
        final l = _lookup(<String, (int, ProbeVerdict)>{
          'DE Stealth': (118, ProbeVerdict.tcpOnly),
        }).of(_row(_reality, <String>['DE Stealth']));
        expect(l.latency.ms, 118);
        expect(l.isUnconfirmed, isTrue);
        expect(l.verdictNote, contains('Проверен только адрес'));
      },
    );

    test('ядро вердикта не прислало — это «не знаю», а не «работает»', () {
      // Сборка старше поля `verdict`. Показать её число наравне с
      // подтверждёнными значило бы вернуть ровно ту ложь, из-за которой узел с
      // отозванным ключом выглядел самым быстрым.
      final l = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (118, ProbeVerdict.unknown),
      }).of(_row(_reality, <String>['DE Stealth']));
      expect(l.isUnconfirmed, isTrue);
      expect(l.verdictNote, contains('Ядро не сказало'));
    });

    test('отказ называет причину и, где есть, живой адрес', () {
      // Самое полезное, что можно сказать про мёртвый вход: адрес отвечает, а
      // вход — нет. Это отличает «оператор сломал узел» от «сеть режет».
      final l = _lookup(
        <String, (int, ProbeVerdict)>{'DE Stealth': (-1, ProbeVerdict.timeout)},
        tcp: const <String, int>{'DE Stealth': 118},
      ).of(_row(_reality, <String>['DE Stealth']));
      expect(l.verdictNote, contains('не уложился в срок'));
      expect(l.verdictNote, contains('118 мс'));
    });

    test('из нескольких отказов называется самый содержательный', () {
      // «Ключ не принят» человеку говорит больше, чем «адрес молчит»: первое
      // ведёт к оплате, второе — к жалобе оператору. Порядок имён на это
      // влиять не должен.
      final l =
          _lookup(<String, (int, ProbeVerdict)>{
            'NL Stealth': (-1, ProbeVerdict.portClosed),
            'DE Stealth': (-1, ProbeVerdict.authRejected),
          }).of(
            _row(
              _reality,
              <String>['NL Stealth', 'DE Stealth'],
              exitKeys: <String>['nl', 'de'],
            ),
          );
      expect(l.verdict, ProbeVerdict.authRejected);
    });

    test('«меряю» ставится только пока замер идёт', () {
      final row = _row(_reality, <String>['DE Stealth']);
      expect(
        _lookup(
          const <String, (int, ProbeVerdict)>{},
          measuring: true,
        ).of(row).latency.source,
        LatencySource.measuring,
      );
      // Замер кончился, числа так и нет: строка обязана вернуться к прочерку,
      // а не остаться в «меряю» навсегда.
      expect(
        _lookup(const <String, (int, ProbeVerdict)>{}).of(row).latency.source,
        LatencySource.none,
      );
    });

    test('имён прокси нет — мерить нечего, и это не «не мерили»', () {
      // Инбаунд, который генератор Clash не выпускает (`naive` на узле 1):
      // имени в теле конфига у него нет, ядро его не назовёт никогда. Строка
      // не показывает даже прочерка — он обещал бы, что число появится.
      final l = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (40, ProbeVerdict.ok),
      }).of(_row(_reality, const <String>[]));
      expect(l.hasProxies, isFalse);
      expect(l, same(InboundLatency.nothingToMeasure));
    });

    test('на нескольких узлах берётся ЛУЧШИЙ ответивший, а не первый', () {
      // Область «весь флот»: одна тройка на трёх машинах. Худший наказал бы
      // строку за далёкую машину, к которой пользователь не подключится, а
      // первый попавшийся зависел бы от порядка ключей.
      final l =
          _lookup(<String, (int, ProbeVerdict)>{
            'DE Stealth': (180, ProbeVerdict.ok),
            'CA Stealth': (90, ProbeVerdict.ok),
            'NL Stealth': (-1, ProbeVerdict.portClosed),
          }).of(
            _row(
              _reality,
              <String>['DE Stealth', 'CA Stealth', 'NL Stealth'],
              exitKeys: <String>['de', 'ca', 'nl'],
            ),
          );
      expect(l.latency.ms, 90);
      expect(l.named, 3);
      expect(l.measured, 3);
      expect(l.answered, 2);
      expect(l.spreadNote, contains('лучший из 2 ответивших'));
      expect(l.spreadNote, contains('всего их 3'));
      // Счёт в ПРОКСИ, а не в узлах: один и тот же вход панель выпускает
      // дважды (прямо и «via 🇷🇺»), и «из 2 узлов» на одной машине было бы
      // выдуманной второй машиной.
      expect(l.spreadNote, isNot(contains('узл')));
      // Вердикт берётся у ТОГО прокси, чьё число показано, а не у худшего.
      expect(l.verdict, ProbeVerdict.ok);
    });

    test('ни один из нескольких не пропустил запрос — отказ строки', () {
      final l =
          _lookup(<String, (int, ProbeVerdict)>{
            'DE Stealth': (-1, ProbeVerdict.tlsUntrusted),
            'CA Stealth': (-1, ProbeVerdict.tlsUntrusted),
          }).of(
            _row(
              _reality,
              <String>['DE Stealth', 'CA Stealth'],
              exitKeys: <String>['de', 'ca'],
            ),
          );
      expect(l.latency.isTimeout, isTrue);
      expect(
        l.spreadNote,
        'Ни один из 2 прокси этого входа не пропустил запрос.',
      );
    });

    test('строка одной машины про разброс молчит', () {
      // Приписывать «лучший из 1» одному узлу — шум, который перестают читать.
      final l = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (180, ProbeVerdict.ok),
      }).of(_row(_reality, <String>['DE Stealth']));
      expect(l.spreadNote, isNull);
      expect(l.isPartial, isFalse);
    });

    test('часть прокси ещё не отвечала — строка знает, что замер неполон', () {
      final l =
          _lookup(<String, (int, ProbeVerdict)>{
            'DE Stealth': (180, ProbeVerdict.ok),
          }).of(
            _row(
              _reality,
              <String>['DE Stealth', 'CA Stealth'],
              exitKeys: <String>['de', 'ca'],
            ),
          );
      expect(l.isPartial, isTrue);
      expect(l.measured, 1);
      expect(l.named, 2);
    });

    test('чужое имя прокси в замер этой строки не попадает', () {
      final l = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (20, ProbeVerdict.ok),
      }).of(_row(_hy2, <String>['DE Hysteria']));
      expect(l.latency.source, LatencySource.none);
    });

    test('оговорка про адрес появляется только там, где такие строки есть', () {
      final rows = <ProtocolRow>[
        _row(_reality, <String>['DE Stealth']),
        _row(_hy2, <String>['DE Speed']),
      ];
      final confirmed = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (180, ProbeVerdict.ok),
        'DE Speed': (96, ProbeVerdict.ok),
      });
      expect(confirmed.anyUnconfirmed(rows), isFalse);

      final mixed = _lookup(<String, (int, ProbeVerdict)>{
        'DE Stealth': (180, ProbeVerdict.ok),
        'DE Speed': (96, ProbeVerdict.tcpOnly),
      });
      expect(mixed.anyUnconfirmed(rows), isTrue);
    });
  });

  group('порядок строк', () {
    InboundLatency at(int tier) => switch (tier) {
      0 => const InboundLatency(
        latency: Latency.fromClient(100),
        verdict: ProbeVerdict.ok,
        tcpMs: -1,
        measured: 1,
        named: 1,
        answered: 1,
      ),
      1 => const InboundLatency(
        latency: Latency.fromClient(10),
        verdict: ProbeVerdict.tcpOnly,
        tcpMs: 10,
        measured: 1,
        named: 1,
        answered: 1,
      ),
      2 => InboundLatency.nothingToMeasure,
      _ => const InboundLatency(
        latency: Latency.fromClient(-1),
        verdict: ProbeVerdict.authRejected,
        tcpMs: 20,
        measured: 1,
        named: 1,
        answered: 0,
      ),
    };

    test('подтверждённое число обгоняет неподтверждённое, даже меньшее', () {
      // 10 мс TCP против 100 мс настоящего запроса. Пустить первое вперёд
      // значило бы вернуть ту самую болезнь, только сортировкой вместо числа.
      expect(inboundTier(selectable: true, latency: at(0)), 0);
      expect(inboundTier(selectable: true, latency: at(1)), 1);
      final confirmedFirst = compareInboundRows(
        (1, true, at(0)),
        (0, true, at(1)),
      );
      expect(confirmedFirst, lessThan(0));
    });

    test('«не мерили» выше отказа: неизвестность — не приговор', () {
      expect(inboundTier(selectable: true, latency: at(2)), 2);
      expect(inboundTier(selectable: true, latency: at(3)), 3);
      final unknownAboveFailure = compareInboundRows(
        (5, true, at(2)),
        (0, true, at(3)),
      );
      expect(unknownAboveFailure, lessThan(0));
    });

    test('невыбираемая строка уходит в конец, как бы быстро ни ответила', () {
      expect(inboundTier(selectable: false, latency: at(0)), 4);
      expect(
        compareInboundRows((0, false, at(0)), (9, true, at(3))),
        greaterThan(0),
      );
    });

    test('равные строки не прыгают: последнее слово за порядком источника', () {
      final sameTier = compareInboundRows((0, true, at(0)), (1, true, at(0)));
      expect(sameTier, lessThan(0));
      final reversed = compareInboundRows((3, true, at(2)), (2, true, at(2)));
      expect(reversed, greaterThan(0));
    });
  });

  group('когда мерить при открытии', () {
    final now = DateTime(2026, 9, 5, 12);

    test('нет чисел — мерим', () {
      expect(
        shouldProbeInbounds(
          measuring: false,
          hasRowsToMeasure: true,
          measured: const <String, int>{},
          measuredAt: null,
          now: now,
        ),
        isTrue,
      );
    });

    test('замер уже идёт — второй проход не запускаем', () {
      // Два параллельных прохода открыли бы вдвое больше соединений и
      // переписали бы результат друг друга.
      expect(
        shouldProbeInbounds(
          measuring: true,
          hasRowsToMeasure: true,
          measured: const <String, int>{},
          measuredAt: null,
          now: now,
        ),
        isFalse,
      );
    });

    test('мерить нечего — не мерим', () {
      // Источник не назвал ни одного имени прокси (молчащая панель): замер
      // вернёт пустой список и ошибку, которой пользователь ничем не поможет.
      expect(
        shouldProbeInbounds(
          measuring: false,
          hasRowsToMeasure: false,
          measured: const <String, int>{},
          measuredAt: null,
          now: now,
        ),
        isFalse,
      );
    });

    test('свежий замер не повторяем, устаревший — повторяем', () {
      final fresh = now.subtract(const Duration(minutes: 9));
      final stale = now.subtract(const Duration(minutes: 11));
      const numbers = <String, int>{'DE Stealth': 180};
      expect(
        shouldProbeInbounds(
          measuring: false,
          hasRowsToMeasure: true,
          measured: numbers,
          measuredAt: fresh,
          now: now,
        ),
        isFalse,
      );
      expect(
        shouldProbeInbounds(
          measuring: false,
          hasRowsToMeasure: true,
          measured: numbers,
          measuredAt: stale,
          now: now,
        ),
        isTrue,
      );
    });
  });

  group('когда мерили — словами', () {
    final now = DateTime(2026, 9, 5, 12);
    test('грубая шкала без тикающих секунд', () {
      expect(
        measuredAgoText(now.subtract(const Duration(seconds: 5)), now),
        'только что',
      );
      expect(
        measuredAgoText(now.subtract(const Duration(minutes: 12)), now),
        '12 мин назад',
      );
      expect(
        measuredAgoText(now.subtract(const Duration(hours: 3)), now),
        '3 ч назад',
      );
      expect(
        measuredAgoText(now.subtract(const Duration(days: 2)), now),
        '2 дн назад',
      );
    });
  });
}

// Меню значка в строке меню: что оно показывает в каждом состоянии.
//
// Это второй интерфейс приложения: при спрятанном окне человек подключается,
// меняет узел и выходит только через него. Проверяем ровно те решения, которые
// иначе видны лишь глазами на живом Маке: какой глагол стоит на кнопке, когда
// выход обязан назвать себя опускающим туннель, что происходит с флотом из
// сорока машин и с профилем, который в системе один.
//
// Плагина здесь нет вовсе: модель чистая.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/tray_menu_model.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Машина подменю с разумными умолчаниями: тест называет только то, что
/// проверяет.
TrayExitItem exit(
  String key, {
  String code = 'NL',
  String? title,
  int? latencyMs,
  bool available = true,
  bool selected = false,
}) => TrayExitItem(
  key: key,
  code: code,
  title: title ?? key,
  latencyMs: latencyMs,
  available: available,
  selected: selected,
);

/// Подписи пунктов верхнего уровня в порядке показа (разделители пропущены).
List<String> labelsOf(TrayMenuSpec spec) => <String>[
  for (final e in spec.entries)
    if (!e.separator) e.label,
];

/// Подписи детей подменю с этим ключом.
List<String> childLabelsOf(TrayMenuSpec spec, String key) => <String>[
  for (final e in spec.find(key)!.children)
    if (!e.separator) e.label,
];

void main() {
  group('заголовок состояния', () {
    test('вне сессии называет стадию и ничего не обещает', () {
      final menu = buildTrayMenu(const TrayMenuInput());

      expect(menu.entries.first.label, DesktopStrings.stageDisconnected);
      expect(
        menu.entries.first.enabled,
        isFalse,
        reason: 'заголовок это ответ, а не действие',
      );
      expect(menu.entries.first.key, isNull);
    });

    test('в сессии называет узел', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(
          stage: VpnStage.connected,
          activeNodeName: 'Amsterdam-1',
        ),
      );

      expect(menu.entries.first.label, 'Подключено · Amsterdam-1');
    });

    test('закрытый доступ поверх поднятого туннеля назван причиной', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(
          stage: VpnStage.connected,
          activeNodeName: 'Amsterdam-1',
          accessBlocked: true,
          blockedReason: 'трафик исчерпан',
        ),
      );

      expect(menu.entries.first.label, contains('доступ закрыт'));
      expect(menu.entries.first.label, contains('трафик исчерпан'));
    });

    test('ошибка доносит причину ядра, а не слово «Ошибка»', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(
          stage: VpnStage.error,
          detail: 'Сервер отказал в подключении',
        ),
      );

      expect(menu.entries.first.label, 'Ошибка: Сервер отказал в подключении');
    });
  });

  group('главное действие', () {
    test('отключённый туннель предлагает подключить', () {
      final menu = buildTrayMenu(const TrayMenuInput());

      expect(menu.find(TrayKeys.toggle)!.label, DesktopStrings.trayConnect);
    });

    test('поднятый туннель предлагает отключить', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(stage: VpnStage.connected),
      );

      expect(menu.find(TrayKeys.toggle)!.label, DesktopStrings.trayDisconnect);
    });

    test('подъём и переподъём предлагают отмену, а не отключение', () {
      for (final stage in <VpnStage>[
        VpnStage.connecting,
        VpnStage.reconnecting,
      ]) {
        final menu = buildTrayMenu(TrayMenuInput(stage: stage));

        expect(
          menu.find(TrayKeys.toggle)!.label,
          DesktopStrings.trayCancelConnect,
          reason: 'туннеля ещё нет, отключать нечего ($stage)',
        );
      }
    });

    test('после ошибки предлагает попробовать снова', () {
      final menu = buildTrayMenu(const TrayMenuInput(stage: VpnStage.error));

      expect(menu.find(TrayKeys.toggle)!.label, DesktopStrings.trayReconnect);
    });
  });

  group('адрес прокси', () {
    test('показан в сессии и копируется', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(
          stage: VpnStage.connected,
          proxyEndpoint: '127.0.0.1:7890',
        ),
      );

      expect(
        menu.find(TrayKeys.proxy)!.label,
        DesktopStrings.trayProxyItem('127.0.0.1:7890'),
      );
    });

    test('вне сессии его нет: слушать на нём уже некому', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(proxyEndpoint: '127.0.0.1:7890'),
      );

      expect(menu.find(TrayKeys.proxy), isNull);
    });
  });

  group('подключений нет вовсе', () {
    late TrayMenuSpec menu;

    setUp(() {
      menu = buildTrayMenu(const TrayMenuInput(noConnections: true));
    });

    test('заголовок говорит об этом прямо', () {
      expect(menu.entries.first.label, DesktopStrings.trayNoConnections);
    });

    test('вместо «Подключить» ведёт заводить подключение', () {
      expect(menu.find(TrayKeys.toggle), isNull);
      expect(
        menu.find(TrayKeys.addConnection)!.label,
        DesktopStrings.trayAddConnection,
      );
    });

    test('подменю выбора сервера не показывается', () {
      expect(
        menu.find(TrayKeys.servers),
        isNull,
        reason: 'подключаться некуда, выбирать узел тем более не из чего',
      );
    });
  });

  group('подменю «Сервер»', () {
    test('начинается с «Авто», отмеченного при снятом пине', () {
      final menu = buildTrayMenu(
        TrayMenuInput(exits: <TrayExitItem>[exit('a', latencyMs: 40)]),
      );

      final auto = menu.find(TrayKeys.serverAuto)!;
      expect(auto.label, DesktopStrings.trayServerAuto);
      expect(auto.checked, isTrue);
    });

    test('закреплённая машина отмечена вместо «Авто»', () {
      final menu = buildTrayMenu(
        TrayMenuInput(
          autoSelected: false,
          exits: <TrayExitItem>[
            exit('a', latencyMs: 40),
            exit('b', latencyMs: 50, selected: true),
          ],
        ),
      );

      expect(menu.find(TrayKeys.serverAuto)!.checked, isFalse);
      expect(menu.find(TrayKeys.server('b'))!.checked, isTrue);
    });

    test('строка машины несёт код страны, имя и задержку', () {
      final menu = buildTrayMenu(
        TrayMenuInput(
          exits: <TrayExitItem>[
            exit('a', code: 'NL', title: 'Amsterdam-1', latencyMs: 42),
          ],
        ),
      );

      expect(menu.find(TrayKeys.server('a'))!.label, 'NL  Amsterdam-1  42 мс');
    });

    test('таймаут и неизвестная задержка числа в строку не приносят', () {
      expect(trayExitLabel(exit('a', title: 'X', latencyMs: -1)), 'NL  X');
      expect(trayExitLabel(exit('a', title: 'X')), 'NL  X');
    });

    test('недоступные машины в меню не попадают', () {
      final menu = buildTrayMenu(
        TrayMenuInput(
          exits: <TrayExitItem>[
            exit('a', latencyMs: 40),
            exit('busy', latencyMs: 10, available: false),
          ],
        ),
      );

      expect(
        menu.find(TrayKeys.server('busy')),
        isNull,
        reason: 'причину в системном меню показать негде',
      );
      expect(menu.find(TrayKeys.server('a')), isNotNull);
    });

    test('порядок по задержке, немеряные уходят вниз', () {
      final menu = buildTrayMenu(
        TrayMenuInput(
          exits: <TrayExitItem>[
            exit('slow', title: 'slow', latencyMs: 200),
            exit('unknown', title: 'unknown'),
            exit('fast', title: 'fast', latencyMs: 20),
            exit('timeout', title: 'timeout', latencyMs: -1),
          ],
        ),
      );

      final labels = childLabelsOf(menu, TrayKeys.servers);
      expect(labels.first, DesktopStrings.trayServerAuto);
      expect(labels[1], contains('fast'));
      expect(labels[2], contains('slow'));
      expect(labels.last, DesktopStrings.trayAllServers);
      // Немеряные ниже меряных, между собой по имени.
      expect(
        labels.indexOf('NL  timeout'),
        lessThan(labels.indexOf('NL  unknown')),
      );
      expect(
        labels.indexOf('NL  slow  200 мс'),
        lessThan(labels.indexOf('NL  timeout')),
      );
    });

    test('флот режется до двенадцати самых быстрых', () {
      final menu = buildTrayMenu(
        TrayMenuInput(
          exits: <TrayExitItem>[
            for (var i = 0; i < 20; i++)
              exit('n$i', title: 'n$i', latencyMs: 100 - i),
          ],
        ),
      );

      final children = menu.find(TrayKeys.servers)!.children;
      final machines = <TrayEntry>[
        for (final e in children)
          if (e.key != null && TrayKeys.nodeKeyOf(e.key!) != null) e,
      ];
      expect(machines, hasLength(kTrayServerLimit));
      // Самая быстрая (n19, 81 мс) осталась, самая медленная (n0) ушла.
      expect(menu.find(TrayKeys.server('n19')), isNotNull);
      expect(menu.find(TrayKeys.server('n0')), isNull);
    });

    test('последним пунктом ведёт на полный экран выбора', () {
      final menu = buildTrayMenu(
        TrayMenuInput(exits: <TrayExitItem>[exit('a', latencyMs: 40)]),
      );

      expect(
        childLabelsOf(menu, TrayKeys.servers).last,
        DesktopStrings.trayAllServers,
      );
    });
  });

  group('подменю «Подключение»', () {
    test('с единственным профилем не показывается', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(
          profiles: <TrayProfileItem>[
            TrayProfileItem(id: 'p1', name: 'Моя подписка', active: true),
          ],
        ),
      );

      expect(
        menu.find(TrayKeys.profiles),
        isNull,
        reason: 'выбор, который ничего не меняет, обещает то, чего нет',
      );
    });

    test('с двумя профилями показывает оба и отмечает активный', () {
      final menu = buildTrayMenu(
        const TrayMenuInput(
          profiles: <TrayProfileItem>[
            TrayProfileItem(id: 'p1', name: 'Моя подписка', active: false),
            TrayProfileItem(id: 'p2', name: 'Аккаунт exarobot', active: true),
          ],
        ),
      );

      expect(childLabelsOf(menu, TrayKeys.profiles), <String>[
        'Моя подписка',
        'Аккаунт exarobot',
      ]);
      expect(menu.find(TrayKeys.profile('p1'))!.checked, isFalse);
      expect(menu.find(TrayKeys.profile('p2'))!.checked, isTrue);
    });
  });

  group('хвост меню', () {
    test('окно, настройки и выход на месте всегда', () {
      final labels = labelsOf(buildTrayMenu(const TrayMenuInput()));

      expect(labels, contains(DesktopStrings.trayOpenWindow()));
      expect(labels, contains(DesktopStrings.traySettings));
      expect(labels.last, DesktopStrings.trayQuit);
    });

    test('при поднятом туннеле выход называет себя опускающим его', () {
      for (final stage in <VpnStage>[
        VpnStage.connected,
        VpnStage.connecting,
        VpnStage.reconnecting,
      ]) {
        final menu = buildTrayMenu(TrayMenuInput(stage: stage));

        expect(
          menu.find(TrayKeys.quit)!.label,
          DesktopStrings.trayQuitAndDisconnect,
          reason:
              'молчаливое «Выйти» обещало бы, что защита останется ($stage)',
        );
      }
    });

    test('после ошибки выход обычный: опускать нечего', () {
      final menu = buildTrayMenu(const TrayMenuInput(stage: VpnStage.error));

      expect(menu.find(TrayKeys.quit)!.label, DesktopStrings.trayQuit);
    });
  });

  group('ключи пунктов', () {
    test('узел и профиль разбираются обратно', () {
      expect(TrayKeys.nodeKeyOf(TrayKeys.server('node-7')), 'node-7');
      expect(TrayKeys.profileIdOf(TrayKeys.profile('p1')), 'p1');
    });

    test('«Авто» не путается с узлом', () {
      expect(
        TrayKeys.nodeKeyOf(TrayKeys.serverAuto),
        isNull,
        reason: 'иначе «Авто» ушло бы в selectNode с ключом «auto»',
      );
      expect(TrayKeys.nodeKeyOf(TrayKeys.allServers), isNull);
      expect(TrayKeys.profileIdOf(TrayKeys.profiles), isNull);
    });
  });
}

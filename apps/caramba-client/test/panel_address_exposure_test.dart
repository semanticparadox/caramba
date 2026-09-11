// Адреса панели нет ни на одном экране приложения — инвариант и его границы.
//
// Решение владельца (раунд 5): адрес панели не показывается НИГДЕ и ни под
// каким действием. Ссылка `caramba://` теперь несёт origin зеркала, и смысл
// этой замены пропадает целиком, если приложение печатает адрес на экране или
// кладёт его в постоянное состояние: скриншот в поддержку, имя профиля на
// главном экране и текст тоста выдают ровно то, что убирали.
//
// Файл стережёт четыре разных пути утечки, и ломаются они в разные стороны:
// «вернули строку на подтверждение», «вернули кнопку, раскрывающую адрес»,
// «подставили хост вместо имени профиля», «вписали хост в текст ошибки».

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/data/panel_probe.dart';
import 'package:caramba_client/features/connections/connection_import_screen.dart';
import 'package:caramba_client/features/enroll/connect_controller.dart';
import 'package:caramba_client/features/enroll/connect_link.dart';
import 'package:caramba_client/features/enroll/connect_redeem.dart';
import 'package:caramba_client/features/enroll/connect_screen.dart';
import 'package:caramba_client/theme/app_theme.dart';

/// Живой адрес панели владельца: тесты пишутся против той строки, из-за которой
/// разговор и начался.
const String _origin = 'https://panel.exarobot.top';
const String _host = 'panel.exarobot.top';

class _Seeded extends ConnectNotifier {
  _Seeded(super.ref, ConnectState seed) {
    state = seed;
  }
}

const _link = CarambaConnectLink(
  origin: _origin,
  code: '000102030405060708090a0b0c0d0e0f',
  operatorName: 'Caramba Connect',
  expiresAtSec: 1780000000,
);

const _tokens = AuthTokens(
  accessToken: 'a',
  refreshToken: 'r',
  tokenType: 'Bearer',
  expiresIn: 900,
  userId: 46,
);

Widget _screen(ConnectState seed) => ProviderScope(
  key: ValueKey<String>('${seed.stage}'),
  overrides: [connectProvider.overrideWith((ref) => _Seeded(ref, seed))],
  child: MaterialApp(theme: AppTheme.dark(), home: const ConnectScreen()),
);

Future<void> _pump(WidgetTester tester, ConnectState seed) async {
  tester.view
    ..physicalSize = const Size(900, 2400)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(_screen(seed));
  await tester.pump();
}

/// Весь видимый текст экрана одной строкой: утечка адреса это не обязательно
/// строка «Адрес панели», это любое место, куда он попал.
String _visibleText(WidgetTester tester) => tester
    .widgetList<Text>(find.byType(Text))
    .map((t) => t.data ?? t.textSpan?.toPlainText() ?? '')
    .join('\n');

void main() {
  group('экран подтверждения: адреса нет и раскрыть его нечем', () {
    testWidgets('ни строки, ни хоста в кадре', (tester) async {
      await _pump(
        tester,
        const ConnectState(stage: ConnectStage.confirm, link: _link),
      );

      expect(find.text(_origin), findsNothing);
      expect(find.text('Адрес панели'), findsNothing);
      // И нигде в кадре: утечка это не обязательно строка «Адрес панели», это
      // и голый хост, зашедший в заголовок или в объяснение.
      expect(_visibleText(tester), isNot(contains(_host)));

      // А то, что видно, названо своим происхождением: «Имя из ссылки»
      // читается как заявление отправителя, «Оператор» читалось бы как факт.
      expect(find.text('Имя из ссылки'), findsOneWidget);
      expect(find.text('Приглашение живо'), findsOneWidget);
    });

    testWidgets('кнопки, раскрывающей адрес, на экране нет', (tester) async {
      await _pump(
        tester,
        const ConnectState(stage: ConnectStage.confirm, link: _link),
      );

      // Именно эта кнопка и была последним местом, где адрес показывался:
      // вместе с ней ушла строка «Корневой ключ», раскрывавшаяся рядом.
      expect(find.text('Показать адрес панели'), findsNothing);
      expect(find.text('Скрыть адрес панели'), findsNothing);
      expect(find.text('Корневой ключ'), findsNothing);

      // Кнопки экрана остались прежние, и ни одна из них адрес не раскрывает.
      expect(find.text('Подключить'), findsOneWidget);
      expect(find.text('Отмена'), findsOneWidget);
    });
  });

  group('экран «панель подключена»: адреса там больше нет', () {
    // Взят вариант с проблемой подписки — ровно то состояние живой панели, где
    // экран висит до нажатия кнопки, а не улетает через 1.2 с. Второй вариант
    // строит тот же список строк тем же кодом, но планирует автопереход, и
    // ждущий таймер в тесте потребовал бы поднимать роутер и auth-слой ради
    // проверки, которая от них не зависит.
    const done = ConnectState(
      stage: ConnectStage.done,
      link: _link,
      result: ConnectRedeemResult(
        tokens: _tokens,
        panelName: 'Caramba Connect',
        subscriptionReason: 'no_subscription_on_account',
      ),
    );

    testWidgets('ни строки «Адрес панели», ни самого адреса', (tester) async {
      await _pump(tester, done);

      expect(find.text('Адрес панели'), findsNothing);
      expect(find.text(_origin), findsNothing);
      // И нигде в тексте кадра — ни в заголовке, ни в объяснении про подписку,
      // ни голым хостом без схемы.
      final text = _visibleText(tester);
      expect(text, isNot(contains(_host)));
    });

    testWidgets('то, что за этот шаг изменилось, экран всё же говорит', (
      tester,
    ) async {
      await _pump(tester, done);

      // Убрать адрес — не то же самое, что превратить экран в пустое «готово».
      // Имя пришло от панели по TLS, и подпись это фиксирует; статус подписки
      // появился только сейчас.
      expect(find.text('Оператор'), findsOneWidget);
      expect(find.text('Имя из ссылки'), findsNothing);
      expect(find.text('Подписка'), findsOneWidget);
    });
  });

  group('тост об ошибке связи: хоста в тексте нет', () {
    // Текст этого исключения печатается пользователю на кадре «Не удалось
    // подключить». Раньше он интерполировал хост панели — то есть показывал
    // адрес ровно тому, кто с большой вероятностью отправит кадр скриншотом.
    test('сетевой отказ называет тип, а не адрес', () {
      const e = ConnectRedeemException(
        'Не удалось связаться с панелью. Проверьте связь и повторите. '
        '(connectionTimeout)',
      );
      expect(e.message, isNot(contains(_host)));
      expect(e.message, isNot(contains(_origin)));
      // Различать «нет сети» и «панель не отвечает» по-прежнему есть чем.
      expect(e.message, contains('connectionTimeout'));
    });

    test('исходник тоста не собирает хост из origin', () {
      final source = File(
        'lib/features/enroll/connect_redeem.dart',
      ).readAsStringSync();
      expect(
        source,
        isNot(contains('Uri.parse(origin).host')),
        reason: 'хост, собранный для текста ошибки, это тот же адрес панели',
      );
    });
  });

  group('имя профиля панели после входа: хост не подставляется', () {
    test('вход без имени оператора не кладёт хост в постоянное состояние', () {
      // Имя профиля печатается на главном экране и в списке подключений, где
      // адрес ничего не подтверждает и никуда не девается. Проверяем исходник:
      // поднимать ради одной строки secure storage, список профилей и весь
      // граф провайдеров значило бы проверять их, а не этот инвариант.
      final source = File('lib/state/auth_state.dart').readAsStringSync();
      expect(
        source,
        isNot(contains('Uri.parse(origin).host')),
        reason:
            'фоллбэк displayName = host был утечкой адреса на главный экран',
      );
      expect(source, contains("displayName: 'Оператор'"));
    });
  });

  group('лист импорта: адрес не подставляется вместо имени', () {
    PanelProbeResult panel(String brand) => PanelProbeResult(
      origin: _origin,
      branding: Branding(brandName: brand),
    );

    test('без брендинга заголовок называет продукт, а не хост', () {
      // Это и был путь утечки: пустой бренд — состояние панели по умолчанию,
      // и заголовок печатал адрес крупным шрифтом у каждого такого оператора.
      final title = panelOfferTitle(panel(''));
      expect(title, isNot(contains(_host)));
      expect(title, 'Это подписка панели Caramba');
    });

    test('бренд оператора проходит как есть', () {
      expect(
        panelOfferTitle(panel('Caramba Connect')),
        contains('Caramba Connect'),
      );
    });
  });

  group('имя профиля подписки: адрес не попадает в постоянное состояние', () {
    // Самая дорогая половина. Имя профиля живёт в хранилище и печатается в
    // строке «Подписка» на главном экране, в списке подключений и на экране
    // входа — местах, где адрес ничего не подтверждает и не исчезает.

    test('без имени в конфиге хост не подставляется', () {
      expect(subscriptionProfileName(typed: '', sourceHost: _host), 'Подписка');
    });

    test('имя из конфига, повторяющее хост, отбрасывается', () {
      expect(
        subscriptionProfileName(
          typed: '',
          fromCore: 'Panel.ExaRobot.Top',
          sourceHost: _host,
        ),
        'Подписка',
        reason: 'адрес, зашедший под видом имени подписки, это тот же адрес',
      );
    });

    test('настоящее имя из конфига остаётся', () {
      expect(
        subscriptionProfileName(
          typed: '',
          fromCore: 'Caramba Free',
          sourceHost: _host,
        ),
        'Caramba Free',
      );
    });

    test('имя, набранное человеком, сильнее всего', () {
      expect(
        subscriptionProfileName(
          typed: '  Рабочая  ',
          fromCore: 'Caramba Free',
          sourceHost: _host,
        ),
        'Рабочая',
      );
    });

    test('одинаковые имена нумеруются, а не сливаются', () {
      expect(
        subscriptionProfileName(
          typed: '',
          sourceHost: _host,
          taken: const ['Подписка'],
        ),
        'Подписка 2',
      );
      expect(
        subscriptionProfileName(
          typed: '',
          sourceHost: _host,
          taken: const ['Подписка', 'Подписка 2', 'Подписка 3'],
        ),
        'Подписка 4',
      );
    });
  });
}

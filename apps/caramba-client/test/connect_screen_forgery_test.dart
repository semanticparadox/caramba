// Экран подтверждения панели: что он рисует и чего нарисовать не может.
//
// Ссылку `caramba://connect` минтит кто угодно — формат опубликован, а хвост
// это контрольная сумма, а не MAC. Имя оператора в ней целиком выбирает
// отправитель, и экран рисует его строкой «слева подпись — справа значение».
// Значение без ограничения строк превращает ОДНО поле в несколько строк,
// выровненных ровно там же, где стоят значения настоящих строк: «Адрес панели
// https://app.exarobot.top» внутри имени заставляет чужую панель выглядеть
// подлинной.
//
// Главная защита стоит на границе разбора (connect_link_test.dart: такое имя
// вообще не доезжает до экрана). Тесты здесь про ВТОРУЮ: даже если поле каким-то
// путём сюда попадёт — из будущего поля ссылки, из ответа панели, из отладочного
// пути — строка экрана вырасти не может.
//
// И отдельно: экран обязан говорить, ЧТО из показанного проверяемо. Адрес
// проверит TLS при первом же запросе, имя не проверит никто.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/features/enroll/connect_controller.dart';
import 'package:caramba_client/features/enroll/connect_link.dart';
import 'package:caramba_client/features/enroll/connect_screen.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart' show InlineBanner;

/// Нотифаер с заранее выставленным состоянием: экран здесь проверяется как
/// отрисовка, а не как поток, и гонять ради этого разбор ссылки значило бы
/// проверять разбор второй раз.
class _Seeded extends ConnectNotifier {
  _Seeded(super.ref, ConnectState seed) {
    state = seed;
  }
}

CarambaConnectLink _link(String operatorName) => CarambaConnectLink(
  origin: 'https://app.exarobot.top',
  code: '000102030405060708090a0b0c0d0e0f',
  operatorName: operatorName,
  expiresAtSec: 1780000000,
);

/// Ключ по содержимому: без него повторный `pumpWidget` в одном тесте
/// переиспользует элемент ProviderScope, оставляет прежний контейнер, и экран
/// продолжает показывать ПЕРВЫЙ seed — сравнение высот молча сравнило бы одну и
/// ту же строку с самой собой.
Widget _screen(ConnectState seed) => ProviderScope(
  key: ValueKey<String>('${seed.stage}/${seed.link?.operatorName}'),
  overrides: [connectProvider.overrideWith((ref) => _Seeded(ref, seed))],
  child: MaterialApp(theme: AppTheme.dark(), home: const ConnectScreen()),
);

Future<void> _pump(WidgetTester tester, ConnectState seed) async {
  tester.view
    ..physicalSize = const Size(900, 2200)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(_screen(seed));
  await tester.pump();
}

/// Высота САМОГО значения, а не строки вокруг него.
///
/// Строка CRow имеет `minHeight: 54`, и три текстовые строки в неё умещаются:
/// высота строки не меняется, а подделка при этом уже нарисована. Мерить надо
/// текст.
double _valueHeight(WidgetTester tester, String text) =>
    tester.getSize(find.text(text)).height;

void main() {
  // Обе строки коротки НАМЕРЕННО. Шрифт flutter_test рисует каждый символ
  // квадратом кегля, столбец значения в этой раскладке ~196 px = 13 символов, и
  // строка длиннее переполняет его: тогда многоточие схлопывает поле обратно в
  // одну строку, и подделка просто не получается. Атакующему это ограничение
  // известно ровно так же, поэтому подделка, которая РАБОТАЕТ, выглядит именно
  // так — коротко и похоже на значение соседней строки «Адрес панели».
  const forged = 'Caramba\nexarobot.top';

  testWidgets('имя с переводами строк не растит строку экрана', (tester) async {
    await _pump(
      tester,
      ConnectState(stage: ConnectStage.confirm, link: _link('Caramba Connect')),
    );
    final plain = _valueHeight(tester, 'Caramba Connect');

    await _pump(
      tester,
      ConnectState(stage: ConnectStage.confirm, link: _link(forged)),
    );
    final grown = _valueHeight(tester, forged);

    expect(
      grown,
      plain,
      reason:
          'поле выросло во вторую строку: «exarobot.top» встало ровно туда, '
          'где стоит значение настоящей строки «Адрес панели»',
    );

    // И сам текст объявлен одной строкой, а не просто уместился случайно.
    final value = tester.widget<Text>(find.text(forged));
    expect(value.maxLines, 1);
    expect(value.overflow, TextOverflow.ellipsis);
  });

  testWidgets('экран разделяет проверяемый адрес и заявленное имя', (
    tester,
  ) async {
    await _pump(
      tester,
      ConnectState(stage: ConnectStage.confirm, link: _link('Caramba Connect')),
    );

    // Подпись у имени называет его происхождение прямо в строке: «Оператор»
    // читается как установленный факт, «Имя из ссылки» — нет.
    expect(find.text('Имя из ссылки'), findsOneWidget);
    expect(find.text('Оператор'), findsNothing);
    expect(find.text('Адрес панели'), findsOneWidget);
    expect(find.text('https://app.exarobot.top'), findsOneWidget);

    // И объяснение, чем одно отличается от другого, а не общее «будьте
    // осторожны».
    final banners = tester
        .widgetList<InlineBanner>(find.byType(InlineBanner))
        .map((b) => b.text)
        .join('\n');
    expect(banners, contains('сертификат'));
    expect(banners, contains('Сверяйте адрес, а не имя'));
    // И прямым текстом: ссылка НЕ зашифрована. Обратное утверждение на этом
    // экране появиться не может — шифровать её нечем.
    expect(banners, contains('не зашифрована'));
  });

  testWidgets('отвергнутая подделка объясняется, а не показывается', (
    tester,
  ) async {
    await _pump(
      tester,
      const ConnectState(
        stage: ConnectStage.refused,
        failure: ConnectLinkFailure.forgedText,
        detail: 'operator name contains U+000A',
      ),
    );

    expect(find.text(ConnectLinkFailure.forgedText.message), findsOneWidget);
    // «Всё равно продолжить» на этом экране нет и быть не может.
    expect(find.widgetWithText(FilledButton, 'Подключить'), findsNothing);
  });
}

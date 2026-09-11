/// Headless-рендер настоящих экранов приложения в PNG-кадры для демо-анимаций.
///
/// ЗАЧЕМ. Владелец просил серию анимаций «как это работает» на демо-данных.
/// Эмулятор и запись экрана здесь не используются: `flutter test` рисует
/// живые виджеты в оффскрин-буфер через `RenderRepaintBoundary.toImage`, а
/// демо-данные подставляются через те же переопределения провайдеров Riverpod,
/// что и в обычных виджет-тестах. Ничего «руками» поверх виджетов не рисуется.
///
/// Тесты демо запускаются ТОЛЬКО с `--dart-define=CARAMBA_DEMO=1` (иначе
/// `skip`), чтобы CI не тратил на них время. Кадры кладутся в
/// `build/demo/<scene>/NNN.png` + `frames.json`; в GIF их собирает
/// `scripts/demo-gifs.sh`.
///
/// ШРИФТЫ. Тестовый движок Flutter рисует текст шрифтом FlutterTest
/// (квадратики), поэтому настоящие шрифты грузятся через [FontLoader]:
/// Roboto и MaterialIcons из кеша Flutter, системный SF Mono под именем
/// `SF Mono` (так его просит [AppType.monoMd]). Флаги стран и эмодзи в
/// подписях приложения — из Apple Color Emoji, зарегистрированного под
/// последним именем цепочек `AppType.sansFallback`/`monoFallback`
/// (`system-ui` / `monospace`): движок в тестовом режиме отключает
/// per-glyph fallback, и единственный путь дать ему эмодзи — семейство,
/// которое уже стоит в списке fallback у стилей приложения.
library;

import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/theme/app_theme.dart';

const String _demoFlag = String.fromEnvironment('CARAMBA_DEMO');

/// Демо включено флагом `--dart-define=CARAMBA_DEMO=1` (или `=true`).
const bool demoEnabled = _demoFlag == '1' || _demoFlag == 'true';

/// Экран телефона 390×844 при плотности 2: кадры 780×1688.
const Size demoPhysicalSize = Size(780, 1688);
const double demoDpr = 2;

/// Статус-бар телефона (47 логических пикселей), как в home-тестах.
const double demoStatusBarPx = 94;

/// Корень Flutter SDK: из окружения или вверх от `flutter_tester`.
Directory flutterRoot() {
  final env = Platform.environment['FLUTTER_ROOT'];
  if (env != null && env.isNotEmpty) return Directory(env);
  var d = File(Platform.resolvedExecutable).parent;
  for (var i = 0; i < 10; i++) {
    if (File(
      '${d.path}/bin/cache/artifacts/material_fonts/Roboto-Regular.ttf',
    ).existsSync()) {
      return d;
    }
    d = d.parent;
  }
  throw StateError('Flutter SDK не найден от ${Platform.resolvedExecutable}');
}

bool _fontsLoaded = false;

Future<void> _loadFamily(String family, List<String> paths) async {
  final loader = FontLoader(family);
  var any = false;
  for (final p in paths) {
    final f = File(p);
    if (!f.existsSync()) continue;
    any = true;
    final bytes = f.readAsBytesSync();
    loader.addFont(Future<ByteData>.value(ByteData.view(bytes.buffer)));
  }
  if (any) await loader.load();
}

/// Грузит настоящие шрифты один раз на процесс тестов.
Future<void> loadDemoFonts() async {
  if (_fontsLoaded) return;
  _fontsLoaded = true;
  final mf = '${flutterRoot().path}/bin/cache/artifacts/material_fonts';
  final roboto = <String>[
    '$mf/Roboto-Light.ttf',
    '$mf/Roboto-Regular.ttf',
    '$mf/Roboto-Medium.ttf',
    '$mf/Roboto-Bold.ttf',
    '$mf/Roboto-Black.ttf',
  ];
  // Тестовый менеджер шрифтов отдаёт FlutterTest на ЛЮБОЕ незнакомое имя
  // семейства, а SkParagraph берёт первое разрешившееся: у стилей без
  // `fontFamily` (кнопки, тосты) первым в цепочке стоит «SF Pro Text», и
  // латиница с пробелами уходила в квадратики. Поэтому Roboto регистрируется
  // и под первыми именами цепочки [AppType.sansFallback].
  for (final family in <String>['Roboto', 'SF Pro Text', 'SF Pro Display']) {
    await _loadFamily(family, roboto);
  }
  await _loadFamily('MaterialIcons', <String>['$mf/MaterialIcons-Regular.otf']);
  await _loadFamily('SF Mono', <String>['/System/Library/Fonts/SFNSMono.ttf']);
  const emoji = '/System/Library/Fonts/Apple Color Emoji.ttc';
  if (File(emoji).existsSync()) {
    await _loadFamily('system-ui', <String>[emoji]);
    await _loadFamily('monospace', <String>[emoji]);
  }
}

/// Тема приложения для кадров: та же [AppTheme.dark], но стилям, у которых
/// `fontFamily` не задан и которые Material подставляет через СВОЙ
/// `DefaultTextStyle` (кнопки, тосты, подсказки полей, диалоги), явно
/// проставлен Roboto.
///
/// ЗАЧЕМ. Пустое имя семейства тестовый менеджер шрифтов разрешает в
/// FlutterTest, и в кнопках/тостах латиница шла квадратиками, а пробелы —
/// в ширину буквы. Обычные `Text` этим не болеют: они наследуют `bodyMedium`,
/// куда `Typography` уже подмешала `fontFamily: Roboto`. Внешний вид от этого
/// не меняется: на Android приложение и так рисует эти надписи Roboto.
ThemeData demoTheme() {
  final t = AppTheme.dark();
  TextStyle? r(TextStyle? s) => s?.copyWith(fontFamily: 'Roboto');
  ButtonStyle? b(ButtonStyle? s) {
    final ts = s?.textStyle?.resolve(const <WidgetState>{});
    if (s == null || ts == null) return s;
    return s.copyWith(textStyle: WidgetStatePropertyAll<TextStyle?>(r(ts)));
  }

  return t.copyWith(
    filledButtonTheme:
        FilledButtonThemeData(style: b(t.filledButtonTheme.style)),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: b(t.outlinedButtonTheme.style),
    ),
    textButtonTheme: TextButtonThemeData(style: b(t.textButtonTheme.style)),
    snackBarTheme: t.snackBarTheme.copyWith(
      contentTextStyle: r(t.snackBarTheme.contentTextStyle),
    ),
    chipTheme: t.chipTheme.copyWith(labelStyle: r(t.chipTheme.labelStyle)),
    inputDecorationTheme: t.inputDecorationTheme.copyWith(
      hintStyle: r(t.inputDecorationTheme.hintStyle),
      labelStyle: r(t.inputDecorationTheme.labelStyle),
    ),
    dialogTheme: t.dialogTheme.copyWith(
      titleTextStyle: r(t.dialogTheme.titleTextStyle),
      contentTextStyle: r(t.dialogTheme.contentTextStyle),
    ),
    listTileTheme: t.listTileTheme.copyWith(
      titleTextStyle: r(t.listTileTheme.titleTextStyle),
      subtitleTextStyle: r(t.listTileTheme.subtitleTextStyle),
    ),
    appBarTheme: t.appBarTheme.copyWith(
      titleTextStyle: r(t.appBarTheme.titleTextStyle),
    ),
  );
}

/// Окно телефона с статус-баром; сбрасывается в tearDown.
void demoPhone(WidgetTester tester) {
  tester.view
    ..physicalSize = demoPhysicalSize
    ..devicePixelRatio = demoDpr
    ..viewPadding = const FakeViewPadding(top: demoStatusBarPx)
    ..padding = const FakeViewPadding(top: demoStatusBarPx);
  addTearDown(tester.view.reset);
}

/// Записывает кадры одной сцены.
///
/// Экран оборачивается в [wrap] (RepaintBoundary с ключом), каждый [shot]
/// пишет `NNN.png` и запоминает задержку показа; [finish] пишет `frames.json`
/// в форме `[{"file": "000.png", "delay_ms": 800}, …]`.
class DemoRecorder {
  DemoRecorder(this.tester, this.scene) : dir = Directory('build/demo/$scene') {
    if (dir.existsSync()) dir.deleteSync(recursive: true);
    dir.createSync(recursive: true);
    // Тени в тестах по умолчанию выключены (рисуются жёсткой рамкой);
    // для кадров нужен настоящий вид.
    _shadowsBefore = debugDisableShadows;
    debugDisableShadows = false;
    addTearDown(() => debugDisableShadows = _shadowsBefore);
  }

  final WidgetTester tester;
  final String scene;
  final Directory dir;
  final GlobalKey boundaryKey = GlobalKey();
  final List<Map<String, Object>> _frames = <Map<String, Object>>[];
  late final bool _shadowsBefore;

  int get frameCount => _frames.length;

  Widget wrap(Widget app) => RepaintBoundary(key: boundaryKey, child: app);

  /// Снимает текущий кадр и держит его [delayMs] миллисекунд в анимации.
  Future<void> shot(int delayMs) async {
    final index = _frames.length;
    final name = '${index.toString().padLeft(3, '0')}.png';
    await tester.runAsync(() async {
      final rb = boundaryKey.currentContext!.findRenderObject()
          as RenderRepaintBoundary;
      final image = await rb.toImage(pixelRatio: demoDpr);
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      image.dispose();
      File('${dir.path}/$name').writeAsBytesSync(bytes!.buffer.asUint8List());
    });
    _frames.add(<String, Object>{'file': name, 'delay_ms': delayMs});
  }

  /// Продвигает время шагами по [step] и снимает кадр после каждого — для
  /// анимаций (вращение дуги, выезд листа, тост).
  Future<void> animate({
    required int steps,
    Duration step = const Duration(milliseconds: 80),
    int? delayMs,
  }) async {
    for (var i = 0; i < steps; i++) {
      await tester.pump(step);
      await shot(delayMs ?? step.inMilliseconds);
    }
  }

  /// Несколько холостых кадров, чтобы асинхронные провайдеры (профили,
  /// списки) успели прочитаться, и настоящая пауза, чтобы доехали ассеты
  /// (SVG-логотип грузится вне фейкового времени теста).
  Future<void> settle({int pumps = 6}) async {
    for (var i = 0; i < pumps; i++) {
      await tester.pump(const Duration(milliseconds: 16));
    }
    await tester.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 150)),
    );
    await tester.pump(const Duration(milliseconds: 16));
    await tester.pump(const Duration(milliseconds: 16));
  }

  /// Настоящая пауза: для подписей, которые считают секунды по часам
  /// (`DateTime.now()`), фейковое время теста не помогает.
  Future<void> realWait(Duration d) async {
    await tester.runAsync(() => Future<void>.delayed(d));
    await tester.pump(const Duration(milliseconds: 16));
  }

  /// Нажатие с видимым состоянием «палец на элементе».
  Future<void> tapAnimated(
    Finder target, {
    int downSteps = 2,
    int upSteps = 3,
    Duration step = const Duration(milliseconds: 70),
  }) async {
    final gesture = await tester.startGesture(tester.getCenter(target));
    await animate(steps: downSteps, step: step);
    await gesture.up();
    await animate(steps: upSteps, step: step);
  }

  /// Прокрутка пальцем: список едет кадр за кадром, как на телефоне.
  Future<void> dragAnimated(
    Finder target,
    Offset total, {
    int steps = 8,
    Duration step = const Duration(milliseconds: 60),
    int holdMs = 600,
  }) async {
    final gesture = await tester.startGesture(tester.getCenter(target));
    final piece = Offset(total.dx / steps, total.dy / steps);
    for (var i = 0; i < steps; i++) {
      await gesture.moveBy(piece);
      await tester.pump(step);
      await shot(step.inMilliseconds);
    }
    await gesture.up();
    await animate(steps: 3, step: step);
    await tester.pump(const Duration(milliseconds: 400));
    await shot(holdMs);
  }

  /// Набор текста по буквам.
  Future<void> typeAnimated(
    Finder field,
    String text, {
    int delayMs = 90,
  }) async {
    for (var i = 1; i <= text.length; i++) {
      await tester.enterText(field, text.substring(0, i));
      await tester.pump(const Duration(milliseconds: 40));
      await shot(i == text.length ? 700 : delayMs);
    }
  }

  void finish() {
    // Вернуть до проверки инвариантов flutter_test (она идёт раньше tearDown).
    debugDisableShadows = _shadowsBefore;
    File('${dir.path}/frames.json').writeAsStringSync(
      const JsonEncoder.withIndent('  ').convert(_frames),
    );
    stdout.writeln('demo[$scene]: ${_frames.length} кадров -> ${dir.path}');
  }
}

/// Убирает дерево, чтобы таймеры экранов (тикеры, анимации) отменились до
/// проверки «нет висящих таймеров» в конце теста.
Future<void> demoTearDownTree(WidgetTester tester) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump(const Duration(seconds: 2));
}

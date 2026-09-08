// Десктопные настройки: разбор снимка и жизнь через перезапуск.
//
// Проверяется то же свойство, что у AppSettings: снимок, записанный другой
// версией или испорченный руками, читается по полям и падает на дефолты, а не
// роняет старт. Окно обязано открыться при любом содержимом ключа.

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/state/bootstrap_state.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  /// Поднимает контейнер и дожидается гидратации локальных настроек: ровно то,
  /// что при старте делает роутер, придерживая сплеш.
  Future<ProviderContainer> boot() async {
    final container = ProviderContainer();
    addTearDown(container.dispose);
    await container.read(appBootProvider.future);
    return container;
  }

  group('DesktopPrefs.fromJson', () {
    test('пустая карта даёт дефолты', () {
      final p = DesktopPrefs.fromJson(const <String, dynamic>{});

      expect(p.closeToTray, isTrue);
      expect(p.startInTray, isFalse);
      expect(p.launchAtLogin, isFalse);
      expect(p.windowBounds, isNull);
      expect(p.maximized, isFalse);
    });

    test('round-trip сохраняет все поля', () {
      const source = DesktopPrefs(
        closeToTray: false,
        startInTray: true,
        launchAtLogin: true,
        windowBounds: Rect.fromLTWH(120, 64, 1120, 720),
        maximized: true,
      );

      final restored = DesktopPrefs.fromJson(source.toJson());

      expect(restored, source);
      expect(restored.windowBounds, const Rect.fromLTWH(120, 64, 1120, 720));
    });

    test('целые числа в геометрии читаются как double', () {
      final p = DesktopPrefs.fromJson(const {
        'x': 0,
        'y': 25,
        'w': 960,
        'h': 640,
      });

      expect(p.windowBounds, const Rect.fromLTWH(0, 25, 960, 640));
    });

    test('битые поля падают на дефолты независимо друг от друга', () {
      final p = DesktopPrefs.fromJson(const {
        'close_to_tray': 'yes',
        'start_in_tray': 1,
        'launch_at_login': true,
        'maximized': null,
      });

      expect(p.closeToTray, isTrue, reason: 'строка вместо bool');
      expect(p.startInTray, isFalse, reason: 'число вместо bool');
      expect(p.launchAtLogin, isTrue, reason: 'валидное поле уцелело');
      expect(p.maximized, isFalse);
    });

    test('неполная или бессмысленная геометрия отбрасывается целиком', () {
      expect(
        DesktopPrefs.fromJson(const {'x': 10, 'y': 10, 'w': 800}).windowBounds,
        isNull,
        reason: 'три числа из четырёх это мусор, а не геометрия',
      );
      expect(
        DesktopPrefs.fromJson(const {
          'x': 10.0,
          'y': 10.0,
          'w': 0.0,
          'h': 640.0,
        }).windowBounds,
        isNull,
        reason: 'нулевая ширина',
      );
      expect(
        DesktopPrefs.fromJson(const {
          'x': 10.0,
          'y': 10.0,
          'w': double.infinity,
          'h': 640.0,
        }).windowBounds,
        isNull,
        reason: 'бесконечность не размер окна',
      );
      expect(
        DesktopPrefs.fromJson(const {
          'x': 'left',
          'y': 10.0,
          'w': 960.0,
          'h': 640.0,
        }).windowBounds,
        isNull,
      );
    });

    test('снимок без геометрии не пишет пустые ключи', () {
      expect(const DesktopPrefs().toJson().containsKey('x'), isFalse);
    });
  });

  group('desktopPrefsProvider', () {
    test('чистая установка: прячемся в трей, автозапуска нет', () async {
      final c = await boot();

      expect(c.read(desktopPrefsProvider), const DesktopPrefs());
    });

    test('настройки переживают перезапуск', () async {
      final first = await boot();
      final n = first.read(desktopPrefsProvider.notifier);
      n.setCloseToTray(false);
      n.setStartInTray(true);
      n.setLaunchAtLogin(true);
      n.setWindowBounds(
        const Rect.fromLTWH(64, 48, 1200, 800),
        maximized: true,
      );
      // Записи идут через unawaited: даём микротаскам добежать до prefs.
      await Future<void>.delayed(Duration.zero);

      final second = await boot();
      final restored = second.read(desktopPrefsProvider);

      expect(restored.closeToTray, isFalse);
      expect(restored.startInTray, isTrue);
      expect(restored.launchAtLogin, isTrue);
      expect(restored.windowBounds, const Rect.fromLTWH(64, 48, 1200, 800));
      expect(restored.maximized, isTrue);
    });

    test('сброс геометрии стирает запомненный прямоугольник', () async {
      final first = await boot();
      first
          .read(desktopPrefsProvider.notifier)
          .setWindowBounds(const Rect.fromLTWH(0, 0, 1000, 700));
      await Future<void>.delayed(Duration.zero);
      first.read(desktopPrefsProvider.notifier).setWindowBounds(null);
      await Future<void>.delayed(Duration.zero);

      final second = await boot();

      expect(second.read(desktopPrefsProvider).windowBounds, isNull);
      expect(second.read(desktopPrefsProvider).maximized, isFalse);
    });

    test('гидратация догоняет провайдер, прочитанный до конца загрузки', () {
      SharedPreferences.setMockInitialValues(<String, Object>{
        kDesktopPrefsKey:
            '{"close_to_tray":false,"start_in_tray":true,'
            '"launch_at_login":false,"x":10.0,"y":20.0,"w":980.0,"h":660.0,'
            '"maximized":false}',
      });

      return withContainer((container) async {
        // Читаем ДО завершения appBoot: здесь ещё дефолты.
        expect(container.read(desktopPrefsProvider).closeToTray, isTrue);

        await container.read(appBootProvider.future);

        final p = container.read(desktopPrefsProvider);
        expect(p.closeToTray, isFalse);
        expect(p.startInTray, isTrue);
        expect(p.windowBounds, const Rect.fromLTWH(10, 20, 980, 660));
      });
    });

    test('битый JSON под ключом оставляет дефолты', () async {
      SharedPreferences.setMockInitialValues(<String, Object>{
        kDesktopPrefsKey: 'not a json object',
      });

      final c = await boot();

      expect(c.read(desktopPrefsProvider), const DesktopPrefs());
    });
  });
}

/// Прогоняет тело на свежем контейнере, который живёт до конца теста.
Future<void> withContainer(Future<void> Function(ProviderContainer) body) {
  final container = ProviderContainer();
  addTearDown(container.dispose);
  return body(container);
}

/// Контракт канала `com.caramba/vpn`: у КАЖДОГО метода, который может уйти с
/// Dart-стороны, обязан быть нативный обработчик.
///
/// Почему это отдельный тест, а не «и так видно». Метод без нативной ветки не
/// падает громко: платформа отвечает `notImplemented`, Flutter поднимает
/// `MissingPluginException`, вызывающий её ловит — и экран показывает
/// «ядро отчёта не отдаёт». То есть отсутствие моста выглядит РОВНО так же, как
/// честный ответ ядра «я такого не знаю». Именно так `routeReport` доехал до
/// сборки мостом только на FFI: Go считал отчёт, Dart его спрашивал, а в
/// Kotlin-`when` и Swift-`switch` ветки не было, и владелец на телефоне видел
/// не поломку, а «непонятно, работают или нет».
///
/// Тест закрывает КЛАСС ошибки, а не случай:
///
///  1. список упражнений сверяется с самим `VpnConnection` — новый метод
///     контракта роняет тест уже здесь, пока про него ещё никто не забыл;
///  2. имена методов снимаются с ЖИВОГО `MethodChannelVpnConnection` (что он
///     реально шлёт в канал), а не переписываются руками;
///  3. нативные обработчики читаются из исходников всех четырёх мостов, и
///     каждое отправленное имя обязано найтись в каждом из них.
library;

import 'dart:convert';
import 'dart:io';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const method = MethodChannel('com.caramba/vpn');
  // Событийные каналы тоже нужны замоканными: конструктор подписывается на
  // статус, а неотвеченный `listen` уходит во FlutterError.reportError и валит
  // тест по причине, к контракту отношения не имеющей.
  const statusEvents = MethodChannel('com.caramba/vpn/status');
  const trafficEvents = MethodChannel('com.caramba/vpn/traffic');

  late TestDefaultBinaryMessenger messenger;
  final sent = <String>{};

  setUp(() {
    sent.clear();
    messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(method, (call) async {
      sent.add(call.method);
      // Значение ответа не важно: проверяется имя, дошедшее до платформы.
      // Разбор пустого ответа на стороне Dart бросает — вызов обёрнут в try.
      return null;
    });
    messenger.setMockMethodCallHandler(statusEvents, (_) async => null);
    messenger.setMockMethodCallHandler(trafficEvents, (_) async => null);
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(method, null);
    messenger.setMockMethodCallHandler(statusEvents, null);
    messenger.setMockMethodCallHandler(trafficEvents, null);
  });

  // Упражнение на каждый метод контракта. Ключ — имя ЧЛЕНА `VpnConnection`
  // (не имя на проводе): по нему сверяется полнота списка.
  Map<String, Future<void> Function()> exercises(
    MethodChannelVpnConnection<String> c,
  ) =>
      <String, Future<void> Function()>{
        'connect': () => c.connect('node-1'),
        'connectRaw': () =>
            c.connectRaw(raw: 'payload', format: 'auto', label: 'profile'),
        'importSubscription': () =>
            c.importSubscription(raw: 'payload', format: 'auto'),
        'probe': () => c.probe(),
        'setPolicy': () => c.setPolicy(CorePolicy.empty),
        'setTunnelMode': () => c.setTunnelMode(TunnelMode.tun),
        'deviceKeygen': () => c.deviceKeygen(),
        'deviceSign': () => c.deviceSign(Uint8List(0)),
        'deviceAgree': () => c.deviceAgree(peerPublicKey: Uint8List(0)),
        'csmRequestSettings': () =>
            c.csmRequestSettings(want: const <int, Object?>{1: 'x'}),
        'csmState': () => c.csmState(),
        'routeReport': () => c.routeReport(),
        'csmLadder': () => c.csmLadder(),
        'csmEnroll': () => c.csmEnroll(origin: 'https://example.invalid'),
        'csmRefresh': () => c.csmRefresh(),
        'csmSetLadder': () => c.csmSetLadder(order: const <int>[0]),
        'csmAnswerCatalogChange': () => c.csmAnswerCatalogChange(accept: true),
        'csmSelectProfile': () => c.csmSelectProfile('profile'),
        'disconnect': () => c.disconnect(),
        // dispose ничего в канал не шлёт, но входит в контракт: без него список
        // упражнений не был бы полным, а именно полнота здесь и проверяется.
        'dispose': () => c.dispose(),
      };

  MethodChannelVpnConnection<String> build() =>
      MethodChannelVpnConnection<String>(
        describe: (String s) => VpnServerArgs(id: s, name: s),
        rawTarget: (String label) => label,
        // Резолвер нужен, чтобы `connect` дошёл до `configure`: без него ветка
        // конфигурации ядра не отправляется вовсе и выпадает из проверки.
        configResolver: () async => const VpnConfig(
          panelUrl: 'https://panel.invalid',
          subscriptionUuid: 'uuid',
          accessToken: 'jwt',
        ),
      );

  test('список упражнений покрывает весь контракт VpnConnection', () {
    final declared = _contractMembers(
      File('${_packageRoot().path}/lib/src/contract.dart').readAsStringSync(),
    );
    // Геттеры (status/traffic/currentStatus) в канал не ходят и в контракте
    // объявлены без скобок — сюда они не попадают by construction.
    expect(
      declared,
      isNotEmpty,
      reason: 'разбор contract.dart не нашёл ни одного метода интерфейса',
    );

    final covered = exercises(build()).keys.toSet();
    expect(
      declared.difference(covered),
      isEmpty,
      reason:
          'у метода контракта нет упражнения. Добавь его в exercises(), иначе '
          'проверка мостов молча перестанет его касаться — а это и есть та '
          'самая тихая расстыковка, ради которой тест существует.',
    );
  });

  test('MethodChannelVpnConnection шлёт ожидаемый набор имён', () async {
    final c = build();
    for (final run in exercises(c).values) {
      try {
        await run();
      } catch (_) {
        // Заглушка отвечает null, и разбор ответа падает. Имя к этому моменту
        // уже записано — только оно и проверяется.
      }
    }
    // Страховка от обратного дрейфа: если реализация перестанет что-то слать,
    // список нативных веток пройдёт проверку по недостроенному множеству.
    expect(sent, contains('configure'));
    expect(sent, contains('routeReport'));
    expect(sent.length, greaterThanOrEqualTo(19));
  });

  for (final bridge in _bridges()) {
    group(bridge.label, () {
      test('обрабатывает каждый метод, который шлёт Dart', () async {
        final c = build();
        for (final run in exercises(c).values) {
          try {
            await run();
          } catch (_) {}
        }

        final handled = bridge.handled;
        expect(
          handled,
          isNotEmpty,
          reason: 'не разобрался диспетчер в ${bridge.path}',
        );

        final missing = sent.difference(handled).difference(bridge.knownGaps);
        expect(
          missing,
          isEmpty,
          reason: 'Dart шлёт эти методы, а ${bridge.path} про них не знает и '
              'ответит notImplemented. На устройстве это выглядит не как '
              'поломка моста, а как честный ответ ядра «не знаю» — добавь '
              'ветку рядом с соседними.',
        );
      });

      if (bridge.knownGaps.isNotEmpty) {
        test('описанные дыры всё ещё дыры', () {
          // Список дыр обязан ржаветь громко: как только ветку добавят, этот
          // тест упадёт и заставит вычеркнуть запись, а не оставить мост
          // навсегда исключённым из проверки выше.
          expect(
            bridge.handled.intersection(bridge.knownGaps),
            isEmpty,
            reason:
                'в ${bridge.path} появилась ветка, помеченная как отсутствующая. '
                'Убери её из knownGaps — иначе исключение переживёт починку и '
                'закроет мост от проверки.',
          );
        });
      }
    });
  }
}

/// Нативный мост: где лежит диспетчер и какие имена он разбирает.
class _Bridge {
  const _Bridge({
    required this.label,
    required this.path,
    required this.handled,
    this.knownGaps = const <String>{},
  });

  final String label;
  final String path;
  final Set<String> handled;

  /// Методы, которых в этом мосте ЗАВЕДОМО нет, с причиной в комментарии у
  /// места объявления. Не «разрешение», а расписка: второй тест группы падает,
  /// как только дыру закроют, и запись приходится удалить.
  final Set<String> knownGaps;
}

List<_Bridge> _bridges() {
  final root = _packageRoot().path;

  String read(String rel) => File('$root/$rel').readAsStringSync();

  const kotlinPath =
      'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnPlugin.kt';
  const swiftPath = 'darwin/Classes/CarambaVpnPlugin.swift';
  const windowsPath = 'windows/caramba_vpn_plugin.cpp';
  const linuxPath = 'linux/caramba_vpn_plugin.cc';

  return <_Bridge>[
    _Bridge(
      label: 'Android (Kotlin)',
      path: kotlinPath,
      handled: _kotlinBranches(
        _slice(
          read(kotlinPath),
          'when (call.method) {',
          'else -> result.notImplemented()',
          kotlinPath,
        ),
      ),
    ),
    _Bridge(
      label: 'iOS/macOS (Swift)',
      path: swiftPath,
      handled: _swiftCases(
        _slice(
          read(swiftPath),
          'switch call.method {',
          'default:',
          swiftPath,
        ),
      ),
    ),
    _Bridge(
      label: 'Windows (C++)',
      path: windowsPath,
      handled: _matches(
        _slice(
          read(windowsPath),
          'void CarambaVpnPlugin::HandleMethodCall(',
          'result->NotImplemented();',
          windowsPath,
        ),
        RegExp(r'method\s*==\s*"([A-Za-z]\w*)"'),
      ),
      // Десктопный мост зовёт ядро через dlsym/GetProcAddress по таблице в
      // caramba_core_ffi.h, и символа CarambaRouteReport там ещё нет: ветка без
      // него отвечала бы ошибкой загрузки, а не отчётом. Правится вместе с
      // заголовком, отдельно от этого захода.
      knownGaps: const <String>{'routeReport'},
    ),
    _Bridge(
      label: 'Linux (C)',
      path: linuxPath,
      handled: _matches(
        _slice(
          read(linuxPath),
          'static void method_call_cb(',
          'fl_method_not_implemented_response_new',
          linuxPath,
        ),
        RegExp(r'g_strcmp0\s*\(\s*method\s*,\s*"([A-Za-z]\w*)"\s*\)'),
      ),
      // См. Windows: та же таблица символов, та же недостающая запись.
      knownGaps: const <String>{'routeReport'},
    ),
  ];
}

/// Кусок исходника между маркерами. Промах маркера — ошибка теста, а не тихо
/// пустое множество: пустой диспетчер прошёл бы любую проверку.
String _slice(String source, String start, String end, String path) {
  final s = source.indexOf(start);
  if (s < 0) {
    throw StateError('$path: не найдено начало диспетчера ("$start")');
  }
  final e = source.indexOf(end, s);
  if (e < 0) {
    throw StateError('$path: не найден конец диспетчера ("$end")');
  }
  return source.substring(s, e);
}

Set<String> _matches(String source, RegExp re) => <String>{
      for (final m in re.allMatches(source)) m.group(1)!,
    };

/// Ветки Kotlin-`when`: строка вида `"csmState" -> {`.
Set<String> _kotlinBranches(String slice) {
  final out = <String>{};
  for (final line in const LineSplitter().convert(slice)) {
    final t = line.trimLeft();
    if (!t.startsWith('"')) continue;
    final arrow = t.indexOf('->');
    if (arrow < 0) continue;
    out.addAll(
      RegExp(
        r'"([A-Za-z]\w*)"',
      ).allMatches(t.substring(0, arrow)).map((m) => m.group(1)!),
    );
  }
  return out;
}

/// Ветки Swift-`switch`: строка вида `case "csmState":` (в том числе списком).
Set<String> _swiftCases(String slice) {
  final out = <String>{};
  for (final line in const LineSplitter().convert(slice)) {
    final t = line.trimLeft();
    if (!t.startsWith('case ')) continue;
    out.addAll(
      RegExp(r'"([A-Za-z]\w*)"').allMatches(t).map((m) => m.group(1)!),
    );
  }
  return out;
}

/// Имена методов интерфейса `VpnConnection` из его собственного объявления.
///
/// Разбор исходника, а не рукописный список: рукописный отстаёт ровно тогда,
/// когда контракт растёт, то есть в единственный момент, когда проверка нужна.
Set<String> _contractMembers(String source) {
  final start = source.indexOf('abstract interface class VpnConnection');
  if (start < 0) {
    throw StateError('contract.dart: не найдено объявление VpnConnection');
  }
  final body = source.substring(start);
  // Объявления члена сидят ровно на двух пробелах отступа; параметры — на
  // четырёх, комментарии начинаются с `/`. Геттеры без скобок не подходят под
  // шаблон и отсеиваются сами.
  return _matches(
    body,
    RegExp(r'^  [A-Za-z_][\w<>,\[\] ?]*\s+(\w+)\s*\(', multiLine: true),
  );
}

/// Корень пакета caramba_vpn, откуда бы ни запустили тест.
Directory _packageRoot() {
  const marker =
      'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnPlugin.kt';
  var dir = Directory.current;
  for (var i = 0; i < 8; i++) {
    if (File('${dir.path}/$marker').existsSync()) return dir;
    final nested = Directory('${dir.path}/packages/caramba_vpn');
    if (File('${nested.path}/$marker').existsSync()) return nested;
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  throw StateError(
    'не найден корень пакета caramba_vpn от ${Directory.current.path}',
  );
}

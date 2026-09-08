// Android-мост статуса: что он пропускает и чего не выдумывает.
//
// Два разных случая с устройства, оба про одно — про снимок статуса, который
// собирается в Kotlin руками.
//
// 1. ABI v2 ТЕРЯЛСЯ. Go отдаёт в statusJSON activeProxy, mode и mixedPort с
//    самого ABI v2; FFI-путь (macOS) разбирает этот JSON целиком, а Android
//    пересобирал снимок из трёх полей и остальные ронял. На Dart-стороне
//    `activeProxyProvider` был null ВСЕГДА, и строка «Сейчас в туннеле» не
//    появлялась на Android ни разу: приложение показывало выбранный узел, не
//    имея ни одного способа сверить его с тем, что держит ядро.
//
// 2. КЭШ ПЕРЕЖИВАЛ ТУННЕЛЬ. Шина статуса — объект, глобальный на процесс.
//    Приложение закрыли кнопкой «Назад», туннель свернулся, процесс остался в
//    памяти, а в шине остался последний кадр «connected» с моментом подъёма.
//    Следующий запуск получал его первым же событием: «Защищено» и идущий
//    таймер над мёртвым tun0.
//
// Kotlin-юнитов в этом пакете нет (модуль собирается только Gradle'ом), а
// проверять свойство надо: оба случая — это молчаливая потеря, которая на
// экране выглядит не как поломка, а как обычная работа. Поэтому здесь читается
// исходник моста, как это уже делает channel_contract_test.dart для диспетчера
// методов.

import 'dart:io';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

const _contract =
    'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnContract.kt';
const _core = 'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaCore.kt';
const _bus =
    'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnBus.kt';
const _service =
    'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnService.kt';

String _read(String rel) => File('${_packageRoot().path}/$rel').readAsStringSync();

/// Корень пакета caramba_vpn, откуда бы ни запустили тест.
Directory _packageRoot() {
  var dir = Directory.current;
  for (var i = 0; i < 8; i++) {
    if (File('${dir.path}/$_contract').existsSync()) return dir;
    final nested = Directory('${dir.path}/packages/caramba_vpn');
    if (File('${nested.path}/$_contract').existsSync()) return nested;
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  throw StateError(
    'не найден корень пакета caramba_vpn от ${Directory.current.path}',
  );
}

/// Тело функции/класса от маркера до закрывающей его строки того же отступа.
/// Промах маркера — ошибка теста, а не пустая строка: пустое тело прошло бы
/// любую проверку.
String _slice(String source, String start, String end, String path) {
  final s = source.indexOf(start);
  if (s < 0) throw StateError('$path: не найдено начало ("$start")');
  final e = source.indexOf(end, s);
  if (e < 0) throw StateError('$path: не найден конец ("$end")');
  return source.substring(s, e);
}

void main() {
  group('ABI v2 доезжает до канала', () {
    test('снимок объявляет все три поля', () {
      final snapshot = _slice(
        _read(_contract),
        'internal data class CarambaStatusSnapshot(',
        'companion object',
        _contract,
      );
      for (final field in const ['activeProxy', 'mode', 'mixedPort']) {
        expect(
          snapshot,
          contains(field),
          reason:
              'поле $field выпало из снимка — на Dart-стороне оно станет null, '
              'и сверить показанный узел с тем, что держит ядро, будет нечем',
        );
      }
    });

    test('карта канала несёт все три ключа', () {
      // Маркер без списка параметров: у asMap появился аргумент (печать
      // независимого наблюдения), и жёсткая сигнатура сделала бы этот тест
      // ошибкой поиска, а не проверкой ключей.
      final asMap = _slice(
        _read(_contract),
        'fun asMap(',
        'companion object',
        _contract,
      );
      for (final key in const ['activeProxy', 'mode', 'mixedPort']) {
        expect(asMap, contains('"$key"'), reason: 'ключ $key не кладётся в карту');
      }
    });

    test('снимок ядра разбирается целиком, а не на три поля', () {
      final status = _slice(
        _read(_core),
        'fun status(): CarambaStatusSnapshot',
        'fun traffic()',
        _core,
      );
      for (final field in const ['activeProxy', 'mode', 'mixedPort']) {
        expect(
          status,
          contains('"$field"'),
          reason: 'status() не читает $field из statusJSON ядра',
        );
      }
    });

    test('поток опроса не пересобирает снимок, теряя поля', () {
      // Ровно так поле и терялось: сервис читал у ядра полный снимок и
      // публиковал новый, собранный из stage/detail/since.
      final poll = _slice(
        _read(_service),
        'private fun pollLoop(',
        'private fun stopTunnel(',
        _service,
      );
      expect(
        poll,
        isNot(contains('CarambaStatusSnapshot(status.stage')),
        reason:
            'снимок ядра пересобирается вручную — всё, что не перечислено '
            'поимённо, снова потеряется',
      );
      expect(poll, contains('status.copy('));
    });

    test('Dart разбирает карту такой формы полностью', () {
      // Вторая половина того же провода: карта, которую теперь кладёт Kotlin,
      // обязана доехать до полей VpnStatus.
      final status = VpnStatus<String>.fromMap(const <Object?, Object?>{
        'stage': 'connected',
        'detail': null,
        'connectedSinceMs': 1700000000000,
        'activeProxy': 'DE Stealth',
        'mode': 'proxy',
        'mixedPort': 7890,
      });
      expect(status.stage, VpnStage.connected);
      expect(status.activeProxy, 'DE Stealth');
      expect(status.mode, TunnelMode.proxy);
      expect(status.mixedPort, 7890);
    });
  });

  group('кэш статуса не переживает туннель', () {
    test('шина знает про живость сеанса и открывает/закрывает его', () {
      final bus = _read(_bus);
      expect(bus, contains('sessionLive'));
      expect(bus, contains('fun openSession()'));
      expect(bus, contains('fun closeSession()'));
    });

    test('«подключено» без живого сеанса наружу не отдаётся', () {
      final bus = _read(_bus);
      final gate = _slice(
        bus,
        'private fun truthful(',
        'fun setListener(',
        _bus,
      );
      expect(gate, contains('sessionLive'));
      expect(
        gate,
        contains('CarambaStage.CONNECTED'),
        reason: 'именно эта стадия и утверждала защиту над мёртвым tun0',
      );
      expect(gate, contains('CarambaStatusSnapshot.DISCONNECTED'));
      // Оба выхода наружу — повтор новому подписчику и публикация кадра —
      // обязаны идти через проверку. Мимо неё кэш снова начнёт врать.
      expect(bus, contains('fun currentStatus(): CarambaStatusSnapshot = truthful(lastStatus)'));
      final publish = _slice(
        bus,
        'fun publishStatus(',
        'fun publishTraffic(',
        _bus,
      );
      expect(publish, contains('truthful(snapshot)'));
    });

    test('сервис открывает сеанс на подъёме и закрывает на любом выходе', () {
      final service = _read(_service);
      final start = _slice(
        service,
        'private fun startTunnel(',
        'private fun runCore(',
        _service,
      );
      expect(start, contains('CarambaVpnBus.openSession()'));

      final stop = _slice(
        service,
        'private fun stopTunnel(',
        'override fun onDestroy()',
        _service,
      );
      expect(stop, contains('CarambaVpnBus.closeSession()'));

      final destroy = _slice(
        service,
        'override fun onDestroy()',
        'override fun onRevoke()',
        _service,
      );
      expect(
        destroy,
        contains('CarambaVpnBus.closeSession()'),
        reason:
            'путь без stopTunnel не гипотетический: подъём, упавший на '
            'buildInterface, уходит сразу в stopSelf()',
      );
    });

    test('опоздавший кадр опроса не ложится поверх «отключено»', () {
      final poll = _slice(
        _read(_service),
        'private fun pollLoop(',
        'private fun stopTunnel(',
        _service,
      );
      expect(
        poll,
        contains('if (!running.get()) break'),
        reason:
            'между чтением статуса у ядра и публикацией сеанс успевает '
            'закрыться, и кадр «connected» возвращает кэш в подключённое '
            'состояние уже после разбора туннеля',
      );
    });
  });
}

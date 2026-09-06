// Независимый признак под словом «Защищено».
//
// ЧТО СЛУЧИЛОСЬ. Стадия туннеля целиком принадлежала Go-ядру: оно объявляло
// себя подключённым, приложение это повторяло, и сверить утверждение было не с
// чем. Ядро ошибалось — `executor.Shutdown()` у mihomo глобален на процесс, и
// закрытие служебного ядра валило чужой живой туннель, оставляя поднявший его
// движок в StateConnected. Корень устранён, но конструкция «один источник
// утверждает сам о себе» осталась бы прежней. Второй источник — система:
// сеть с транспортом VPN (то же, чем определяется значок в статус-баре) плюс
// локальный TUN-интерфейс.
//
// ЧЕГО ЗДЕСЬ БОЯТСЯ БОЛЬШЕ ВСЕГО. Ошибка в другую сторону — «Отключено» на
// живом туннеле — опаснее исходного дефекта: человек полезет включать защиту
// заново, оборвёт работающий туннель и в промежутке выпустит трафик открытым.
// Поэтому «не знаю» это полноценный третий ответ, и почти всё здесь проверяет
// именно его.
//
// Kotlin-юнитов в этом пакете нет (модуль собирается только Gradle'ом), поэтому
// нативная половина проверяется чтением исходника — как это уже делают
// channel_contract_test.dart и android_status_bridge_test.dart.

import 'dart:io';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

const _manifest = 'android/src/main/AndroidManifest.xml';
const _witness =
    'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaTunnelWitness.kt';
const _plugin =
    'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnPlugin.kt';
const _service =
    'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnService.kt';
const _contract =
    'android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnContract.kt';

String _read(String rel) =>
    File('${_packageRoot().path}/$rel').readAsStringSync();

Directory _packageRoot() {
  var dir = Directory.current;
  for (var i = 0; i < 8; i++) {
    if (File('${dir.path}/$_manifest').existsSync()) return dir;
    final nested = Directory('${dir.path}/packages/caramba_vpn');
    if (File('${nested.path}/$_manifest').existsSync()) return nested;
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  throw StateError(
    'не найден корень пакета caramba_vpn от ${Directory.current.path}',
  );
}

/// Тело от маркера до конца; промах маркера — ошибка теста, а не пустая
/// строка: пустое тело прошло бы любую проверку.
String _slice(String source, String start, String end, String path) {
  final s = source.indexOf(start);
  if (s < 0) throw StateError('$path: не найдено начало ("$start")');
  final e = source.indexOf(end, s);
  if (e < 0) throw StateError('$path: не найден конец ("$end")');
  return source.substring(s, e);
}

void main() {
  group('провод несёт наблюдение', () {
    test('карта разбирается в present/absent/unknown', () {
      VpnStatus<String> parse(Object? witness) =>
          VpnStatus<String>.fromMap(<Object?, Object?>{
            'stage': 'connected',
            'connectedSinceMs': 1700000000000,
            if (witness != null) 'tunnelWitness': witness,
          });

      expect(parse('present').witness, TunnelWitness.present);
      expect(parse('absent').witness, TunnelWitness.absent);
      expect(parse('unknown').witness, TunnelWitness.unknown);
    });

    test('отсутствие ключа и чужой тип читаются как «не знаю»', () {
      // Мост без наблюдения (iOS, desktop, сборка старее этой правки) обязан
      // давать unknown. Прочитать пустоту как absent значило бы гасить щит на
      // всех платформах разом.
      final noKey = VpnStatus<String>.fromMap(const <Object?, Object?>{
        'stage': 'connected',
      });
      expect(noKey.witness, TunnelWitness.unknown);

      for (final junk in <Object?>[null, 7, true, <String>['absent']]) {
        final status = VpnStatus<String>.fromMap(<Object?, Object?>{
          'stage': 'connected',
          'tunnelWitness': junk,
        });
        expect(
          status.witness,
          TunnelWitness.unknown,
          reason: 'значение $junk не является наблюдением отсутствия',
        );
      }
    });

    test('незнакомое слово не превращается в приговор', () {
      expect(TunnelWitness.fromWire('ABSENT'), TunnelWitness.unknown);
      expect(TunnelWitness.fromWire('нет'), TunnelWitness.unknown);
      expect(TunnelWitness.fromWire(''), TunnelWitness.unknown);
    });

    test('снимок по умолчанию ничего не наблюдает', () {
      const s = VpnStatus<String>.disconnected();
      expect(s.witness, TunnelWitness.unknown);
      expect(
        const VpnStatus<String>(stage: VpnStage.connected).witness,
        TunnelWitness.unknown,
      );
    });

    test('copyWith сохраняет наблюдение, а не теряет его', () {
      const s = VpnStatus<String>(
        stage: VpnStage.connected,
        witness: TunnelWitness.absent,
      );
      expect(s.copyWith(detail: 'x').witness, TunnelWitness.absent);
      expect(
        s.copyWith(witness: TunnelWitness.present).witness,
        TunnelWitness.present,
      );
    });
  });

  group('нативная сторона умеет ответить «не знаю»', () {
    test('разрешение на чтение состояния сети объявлено', () {
      // Без ACCESS_NETWORK_STATE getNetworkCapabilities начинает отдавать null,
      // наблюдение вырождается в unknown — и щит остаётся гореть над мёртвым
      // туннелем. Разрешение обычное, у пользователя ничего не спрашивается.
      expect(
        _read(_manifest),
        contains('android.permission.ACCESS_NETWORK_STATE'),
        reason: 'независимому источнику нечем ответить',
      );
    });

    test('вердикт троичный', () {
      final src = _read(_witness);
      for (final v in const ['PRESENT', 'ABSENT', 'UNKNOWN']) {
        expect(src, contains(v));
      }
    });

    test('«нет» требует ДВУХ состоявшихся отрицаний', () {
      final observe = _slice(
        _read(_witness),
        'private fun observe(',
        'private fun vpnTransport(',
        _witness,
      );
      expect(
        observe,
        contains(
          'if (transport == Verdict.ABSENT && iface == Verdict.ABSENT) return Verdict.ABSENT',
        ),
        reason:
            'одного отрицания мало: приложение исключает себя из туннеля '
            '(addDisallowedApplication), и для собственного процесса активной '
            'сетью остаётся Wi-Fi даже при живом VPN',
      );
      expect(
        observe,
        contains('return Verdict.ABSENT\n        return Verdict.UNKNOWN\n    }'),
        reason:
            'выход по умолчанию обязан быть «не знаю»: всё, что не два '
            'состоявшихся отрицания, приговором не является',
      );
    });

    test('каждое наблюдение отдельно умеет сказать «не знаю»', () {
      final src = _read(_witness);
      final transport = _slice(
        src,
        'private fun vpnTransport(',
        'private fun tunInterface(',
        _witness,
      );
      expect(
        transport,
        contains('if (answered) Verdict.ABSENT else Verdict.UNKNOWN'),
        reason:
            'перебор, не увидевший НИ ОДНОЙ характеристики, ничего не '
            'наблюдал — это не наблюдение отсутствия',
      );
      expect(
        transport,
        contains('catch (_: Throwable) {\n            Verdict.UNKNOWN'),
        reason: 'исключение — молчание, а не приговор',
      );

      final iface = _slice(
        src,
        'private fun tunInterface(',
        'private fun carriesTunAddress(',
        _witness,
      );
      expect(
        iface,
        contains('if (answered) Verdict.ABSENT else Verdict.UNKNOWN'),
      );
    });

    test('адрес TUN — один на сервис и свидетеля', () {
      // Вторая копия адреса разошлась бы с первой молча, и свидетель перестал
      // бы находить живой туннель — то есть начал бы врать в опасную сторону.
      expect(_read(_witness), contains('const val ADDRESS = "172.19.0.1"'));
      final service = _read(_service);
      expect(service, contains('CarambaTun.ADDRESS'));
      expect(
        service,
        isNot(contains('"172.19.0.1"')),
        reason: 'адрес продублирован в сервисе',
      );
    });

    test('наблюдение снимается на выходе в Dart, а не кэшируется в снимке', () {
      // Снимок кэшируется шиной и копируется поллером: наблюдение, пролежавшее
      // в нём такт, было бы такой же памятью о прошлом, как то самое
      // «Защищено».
      expect(
        _read(_contract),
        contains('fun asMap(witness: String = "unknown")'),
      );
      final plugin = _read(_plugin);
      expect(plugin, contains('CarambaTunnelWitness.verdict(appContext).wire'));
      // Три выхода наружу: живой кадр, повтор новому подписчику и прямой ответ
      // на `status`. Мимо печати любой из них снова начнёт утверждать защиту.
      expect(
        plugin,
        contains('statusSink?.success(witnessed(snapshot))'),
        reason: 'живой кадр без наблюдения',
      );
      expect(
        plugin,
        contains('events?.success(witnessed(CarambaVpnBus.currentStatus()))'),
        reason: 'повтор новому движку Flutter — тот самый кадр, что и врал',
      );
      expect(
        plugin,
        contains('result.success(witnessed(CarambaVpnBus.currentStatus()))'),
        reason: 'прямой вопрос Home при запуске и возвращении из фона',
      );
    });
  });

  group('поднялись без адаптера', () {
    test('признак — растущий tx_dropped при нулевом rx', () {
      final src = _read(_witness);
      final watch = _slice(
        src,
        'internal class CarambaTunWatch',
        'private fun counter(',
        _witness,
      );
      expect(watch, contains('rx_bytes'));
      expect(watch, contains('tx_dropped'));
      expect(
        watch,
        contains('alive = true'),
        reason:
            'адаптер, ответивший хоть раз, живой — дальше судить не о чем',
      );
      expect(
        watch,
        contains('GRACE_MS'),
        reason: 'сразу после подъёма нулевой rx законен',
      );
    });

    test('нечитаемые счётчики ничего не решают', () {
      final counter = _slice(
        _read(_witness),
        'private fun counter(',
        '\n}',
        _witness,
      );
      expect(counter, contains('if (!f.canRead()) return null'));
      expect(counter, contains('catch (_: Throwable) {\n            null'));
    });

    test('сервис опускает такой туннель и называет причину', () {
      final poll = _slice(
        _read(_service),
        'private fun pollLoop(',
        'private fun stopTunnel(',
        _service,
      );
      expect(poll, contains('tun.unserviced()'));
      expect(
        poll,
        contains(
          'stopTunnel(CarambaStage.ERROR, CarambaFailureReason.TUN_NOT_SERVICED)',
        ),
        reason:
            'оставить туннель висеть значит оставить человека без сети: весь '
            'трафик уходит в трубу без читателя',
      );
    });

    test('машинные причины совпадают по обе стороны провода', () {
      final kotlin = _read(_witness);
      expect(
        kotlin,
        contains('const val NO_VPN_TRANSPORT = "${VpnFailureReason.noVpnTransport}"'),
      );
      expect(
        kotlin,
        contains('const val TUN_NOT_SERVICED = "${VpnFailureReason.tunNotServiced}"'),
      );
    });
  });
}

/// Шов `configure` обязан переносить в ядро ВСЮ сессию, а не один её
/// короткоживущий кусок.
///
/// Регресс на поломку, которая жила на всех пяти мостах сразу: на провод уходил
/// только access-токен. Он живёт ~15 минут, ядру нечем было его продлить, и
/// через четверть часа каждый его запрос к панели получал 401 без пути назад —
/// «api: загрузка узлов подписки для замера: …» на отлежавшемся телефоне.
library;

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const channel = MethodChannel('com.caramba/vpn');
  final List<MethodCall> calls = <MethodCall>[];

  setUp(() {
    calls.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return null;
        });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  Map<Object?, Object?> argsOf(MethodCall c) =>
      (c.arguments as Map).cast<Object?, Object?>();

  group('шов сессии', () {
    test('configure кладёт на провод refresh и срок жизни access', () async {
      final expiry = DateTime.fromMillisecondsSinceEpoch(1893456000 * 1000);
      await CarambaVpn.instance.configure(
        panelUrl: 'https://panel.example',
        subscriptionId: 'sub-uuid',
        accessToken: 'access-1',
        refreshToken: 'refresh-1',
        accessExpiry: expiry,
      );

      expect(calls.single.method, 'configure');
      final args = argsOf(calls.single);
      expect(args['refreshToken'], 'refresh-1');
      expect(args['accessExpiryUnix'], 1893456000);
      // Ключ подписки остаётся каноническим: нативные стороны читают его первым.
      expect(args['subscriptionUuid'], 'sub-uuid');
      expect(args['accessToken'], 'access-1');
    });

    test('без refresh и срока провод несёт пустую строку и 0, а не null',
        () async {
      // null на проводе означал бы «ключа нет» и на трёх из пяти мостов читался
      // бы как отсутствие аргумента; 0 — это явное «срок неизвестен», по
      // которому ядро разбирает claim exp самого JWT.
      await CarambaVpn.instance.configure(
        panelUrl: 'https://panel.example',
        subscriptionId: 'sub-uuid',
        accessToken: 'access-1',
      );

      final args = argsOf(calls.single);
      expect(args['refreshToken'], '');
      expect(args['accessExpiryUnix'], 0);
    });

    test('VpnConfig.toArgs и фасад кладут на провод одно и то же', () async {
      const cfg = VpnConfig(
        panelUrl: 'https://panel.example',
        subscriptionUuid: 'sub-uuid',
        accessToken: 'access-1',
        refreshToken: 'refresh-1',
      );
      await CarambaVpn.instance.configure(
        panelUrl: cfg.panelUrl,
        subscriptionId: cfg.subscriptionUuid,
        accessToken: cfg.accessToken,
        refreshToken: cfg.refreshToken,
        accessExpiry: cfg.accessExpiry,
      );
      expect(argsOf(calls.single), cfg.toArgs());
    });

    test('равенство VpnConfig видит смену refresh', () {
      const base = VpnConfig(
        panelUrl: 'https://panel.example',
        subscriptionUuid: 'sub-uuid',
        accessToken: 'access-1',
        refreshToken: 'refresh-1',
      );
      const rotated = VpnConfig(
        panelUrl: 'https://panel.example',
        subscriptionUuid: 'sub-uuid',
        accessToken: 'access-1',
        refreshToken: 'refresh-2',
      );
      // Мосты пропускают повторный `configure`, когда шов «не изменился». Пока
      // сравнение писалось руками поле за полем, ротация refresh не доезжала.
      expect(base == rotated, isFalse);
      expect(base == base, isTrue);
    });
  });
}

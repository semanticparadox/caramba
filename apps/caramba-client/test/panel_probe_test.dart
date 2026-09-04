import 'dart:typed_data';

import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/panel_probe.dart';

class _Adapter implements HttpClientAdapter {
  _Adapter(this.status, this.body);
  final int status;
  final String body;
  int calls = 0;

  @override
  Future<ResponseBody> fetch(RequestOptions options, Stream<Uint8List>? _, Future<void>? __) async {
    calls++;
    return ResponseBody.fromString(
      body,
      status,
      headers: {
        Headers.contentTypeHeader: [Headers.jsonContentType],
      },
    );
  }

  @override
  void close({bool force = false}) {}
}

Dio _dio(_Adapter a) => Dio(BaseOptions(validateStatus: (s) => s != null && s < 500))
  ..httpClientAdapter = a;

void main() {
  group('origin ссылки подписки', () {
    test('https с портом сохраняется', () {
      expect(panelOriginOf('https://p.example:8443/sub/uuid?x=1'), 'https://p.example:8443');
    });
    test('сырой конфиг и мусор не дают origin', () {
      expect(panelOriginOf('vless://uuid@host:443'), isNull);
      expect(panelOriginOf('proxies:\n  - name: x'), isNull);
      expect(panelOriginOf(''), isNull);
    });
  });

  group('распознавание панели Caramba', () {
    test('панель отвечает брендингом: подключение предлагается', () async {
      final a = _Adapter(200, '{"brand_name":"Панель X","bot_url":"https://t.me/b","enabled":true}');
      final r = await probeCarambaPanel('https://p.example/sub/u', client: _dio(a));
      expect(r, isNotNull);
      expect(r!.origin, 'https://p.example');
      expect(r.brandName, 'Панель X');
      expect(a.calls, 1);
    });

    test('чужой сервер с посторонним JSON панелью не считается', () async {
      final a = _Adapter(200, '{"hello":"world"}');
      expect(await probeCarambaPanel('https://other.example/sub/u', client: _dio(a)), isNull);
    });

    test('ошибка сети не ломает импорт: просто нет панели', () async {
      final a = _Adapter(404, '');
      expect(await probeCarambaPanel('https://p.example/sub/u', client: _dio(a)), isNull);
    });

    // Раньше здесь ожидался хост: геттер подставлял его вместо пустого бренда,
    // и адрес панели уезжал в заголовок листа импорта у каждого оператора,
    // который брендинг не заполнял. Подстановки больше нет — решение, чем
    // заменить отсутствующее имя, принимает вызывающий, и объясняет его у себя
    // (panel_address_exposure_test.dart).
    test('без имени бренда имени нет, а хост не подставляется', () async {
      final a = _Adapter(200, '{"bot_url":"https://t.me/b"}');
      final r = await probeCarambaPanel('https://p.example/sub/u', client: _dio(a));
      expect(r!.brandName, isEmpty);
      expect(r.origin, 'https://p.example');
    });
  });
}

// Одно поле обязано разбирать ВСЁ, что раньше разбирали три раздела.
//
// Экран подключения свёлся к одному полю: разделов «Подписка» / «Панель
// Caramba» / «Код из бота» больше нет, и решение, к какому из них относится
// вставленная строка, приложение теперь принимает само. Значит, цена ошибки
// разбора выросла: раньше человек мог обойти неверную догадку, выбрав раздел
// руками, — теперь обходить нечем.
//
// Эти тесты стерегут две вещи:
//   * каждый вход, который работал до перестройки (caramba://, приглашение
//     энроллмента, ссылка подписки, обёртка carambaconnect://import, сырой
//     конфиг, одиночный URI), продолжает попадать туда же, куда и попадал;
//   * классификатор не разъезжается с [DeepLinkHandler.targetOf] — той же
//     развилкой, по которой ТА ЖЕ САМАЯ строка приходит от операционной
//     системы. Разъезд означал бы, что ссылка из мессенджера работает, а она
//     же из буфера обмена — нет, и заметить это можно только руками.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/features/connections/entry_classifier.dart';
import 'package:caramba_client/router/deep_links.dart';
import 'package:caramba_client/router/routes.dart';

/// Живая ссылка подключения (тот же вектор, что в connect_import_path_test).
const String _connectLink =
    'caramba://connect?d=8D5320D405W1GT3MEHR76EHF5XGQ0W1ECNW62WKFC9QQ8BKMDXR0'
    '4M00041061050R3GG28A1C60T3GF0DQM6RBJC5PP4R908DQPWVK5CDT0A6KA32JG0063SHSG';

const String _enrollLink =
    'carambaconnect://enroll?panel=https://panel.example&code=ABC123';

/// Приглашение CSM: код и пин — по 20 символов base32 Crockford, иначе разбор
/// CSM отступает старому парсеру, и параметр `k` теряется.
const String _csmCode = 'ABCDEFGH1234567890JK';
const String _enrollLinkWithPin =
    'carambaconnect://enroll?panel=https://panel.example'
    '&code=$_csmCode&k=$_csmCode';

const String _importLink =
    'carambaconnect://import?url=https%3A%2F%2Fsub.example%2Fs%2Fabc';

const String _clashYaml = '''
proxies:
  - name: "probe"
    type: socks5
    server: 127.0.0.1
    port: 1080
''';

void main() {
  group('куда ведёт вставленная строка', () {
    test('пустая строка и пробелы — это пусто, а не конфиг', () {
      expect(classifyEntry('').kind, EntryKind.empty);
      expect(classifyEntry('   \n  ').kind, EntryKind.empty);
    });

    test('caramba:// ведёт на подтверждение приглашения', () {
      final c = classifyEntry(_connectLink);
      expect(c.kind, EntryKind.connectLink);
      expect(c.isLink, isTrue);
      expect(c.target, startsWith(AppRoute.connect));
      expect(c.target, contains('link='));
    });

    test('пробелы вокруг ссылки подключения не мешают', () {
      expect(classifyEntry('  $_connectLink \n').kind, EntryKind.connectLink);
    });

    test('приглашение энроллмента ведёт в энроллмент', () {
      final c = classifyEntry(_enrollLink);
      expect(c.kind, EntryKind.enrollLink);
      expect(c.target, startsWith(AppRoute.enroll));
    });

    test('пин ссылки (k) доезжает до цели, а не теряется по дороге', () {
      // Потеря k превращает закреплённый энроллмент в незакреплённый молча:
      // экран личности оператора перестал бы отличать пин из ссылки от пина,
      // продиктованного вне полосы. Разбор CSM обязан идти раньше старого.
      final c = classifyEntry(_enrollLinkWithPin);
      expect(c.kind, EntryKind.enrollLink);
      expect(c.target, contains('k=$_csmCode'));
    });

    test('carambaconnect://import отдаёт ВНУТРЕННИЙ адрес, а не обёртку', () {
      // В профиль уезжает `source`, и по нему же работает «Обновить подписку».
      // Запиши туда диплинк — обновление качало бы саму ссылку-обёртку.
      final c = classifyEntry(_importLink);
      expect(c.kind, EntryKind.subscriptionUrl);
      expect(c.url, 'https://sub.example/s/abc');
    });

    test('голая ссылка подписки — это ссылка подписки', () {
      final c = classifyEntry('https://sub.example/sub/uuid');
      expect(c.kind, EntryKind.subscriptionUrl);
      expect(c.url, 'https://sub.example/sub/uuid');
    });

    test('одиночный vless:// — это конфиг, его разбирает ядро', () {
      final c = classifyEntry('vless://uuid@host.example:443?type=tcp#node');
      expect(c.kind, EntryKind.configText);
      expect(c.url, isNull);
    });

    test('clash YAML и sing-box JSON — это конфиг', () {
      expect(classifyEntry(_clashYaml).kind, EntryKind.configText);
      expect(classifyEntry('{"outbounds":[]}').kind, EntryKind.configText);
    });

    test('base64-список — это конфиг, формат определит ядро', () {
      expect(
        classifyEntry('dmxlc3M6Ly9hYmNAaG9zdDo0NDM=').kind,
        EntryKind.configText,
      );
    });
  });

  group('отказ остаётся объяснимым', () {
    test('http:// внутри carambaconnect://import называет причину', () {
      // Без этой ветки строка уехала бы в ядро как «конфиг» и вернулась
      // ошибкой разбора YAML — то есть человек чинил бы не то.
      final c = classifyEntry('carambaconnect://import?url=http://sub.example');
      expect(c.kind, EntryKind.refused);
      expect(c.refusalMessage, contains('http://'));
    });

    test('приглашение энроллмента без кода называет причину', () {
      final c = classifyEntry(
        'carambaconnect://enroll?panel=https://panel.example',
      );
      expect(c.kind, EntryKind.refused);
      expect(c.refusalMessage, isNotNull);
      expect(c.refusalMessage, isNot(isEmpty));
    });

    test('caramba:// с чужим действием не выдаётся за конфиг', () {
      final c = classifyEntry('caramba://something?x=1');
      expect(c.kind, EntryKind.refused);
    });
  });

  group('вход из буфера и вход от системы разбираются одинаково', () {
    // ЭТО И ЕСТЬ ГЛАВНЫЙ ГЕЙТ. Одна и та же строка приходит двумя дверями:
    // операционная система доставляет её как deeplink, человек вставляет её из
    // буфера. Пока обе двери ведут в одно место, поле можно оставить одно.
    for (final raw in const [_connectLink, _enrollLink, _enrollLinkWithPin]) {
      test('цель совпадает с deeplink-хендлером: ${raw.substring(0, 24)}…', () {
        expect(classifyEntry(raw).target, DeepLinkHandler.targetOf(raw));
      });
    }

    test('ссылка импорта: адрес тот же, что вытащил бы хендлер', () {
      final target = DeepLinkHandler.targetOf(_importLink);
      expect(target, isNotNull);
      expect(
        Uri.parse(target!).queryParameters['url'],
        classifyEntry(_importLink).url,
      );
    });
  });

  test('энроллмент никогда не открывается БЕЗ кода', () {
    // Тупик, из-за которого владелец не смог добавить свою подписку, выглядел
    // так: экран уводил на «введите инвайт-код» без кода, а выпускать эти коды
    // живой панели было нечем. Классификатор — единственное место, откуда
    // экран подключения теперь берёт адрес энроллмента, поэтому гарантия
    // «без кода туда не ведём» живёт здесь.
    for (final raw in const [
      _enrollLink,
      _enrollLinkWithPin,
      'carambaconnect://enroll?panel=https://panel.example',
      'carambaconnect://enroll?panel=https://panel.example&code=',
    ]) {
      final c = classifyEntry(raw);
      if (c.kind != EntryKind.enrollLink) continue;
      final code = Uri.parse(c.target!).queryParameters['code'] ?? '';
      expect(code, isNot(isEmpty), reason: 'цель без кода: ${c.target}');
    }
  });
}

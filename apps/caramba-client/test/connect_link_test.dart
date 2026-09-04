// Разбор ссылки `caramba://connect?d=<armor>`.
//
// Раскладка байт нормативна и общая с панелью
// (`apps/caramba-panel/src/connect_link.rs`). Поэтому здесь стоят те же
// золотые векторы, что закреплены тестом панели: если приложение и панель
// разойдутся хоть на байт, ссылка перестанет открываться у живых людей, и
// заметить это по логам будет уже поздно.
//
// Отдельно проверяется ПОРЯДОК проверок: контрольная сумма считается ДО разбора
// CBOR. Иначе испорченный при копировании байт всплывает как «неверный формат»,
// и человеку советуют просить новую ссылку там, где достаточно скопировать
// старую целиком.

import 'dart:convert' show utf8;
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_vpn/csm.dart' show base32CrockfordEncode, sha256;

import 'package:caramba_client/features/enroll/connect_link.dart';

/// Золотой вектор панели: origin app.exarobot.top, код 000102..0f, оператор
/// "Caramba Connect", без идентификатора корневого ключа, срок 1780000000.
const String _goldenEnvelopeHex =
    '434a3101'
    'a4'
    '017818'
    '68747470733a2f2f6170702e657861726f626f742e746f70'
    '0250'
    '000102030405060708090a0b0c0d0e0f'
    '036f'
    '436172616d626120436f6e6e656374'
    '051a'
    '6a18a500'
    '00c3cc73';

const String _goldenLink =
    'caramba://connect?d=8D5320D405W1GT3MEHR76EHF5XGQ0W1ECNW62WKFC9QQ8BKMDXR0'
    '4M00041061050R3GG28A1C60T3GF0DQM6RBJC5PP4R908DQPWVK5CDT0A6KA32JG0063SHSG';

/// Второй вектор, снятый с формы живой панели: другой origin, другое имя
/// оператора, другой срок. Он ловит то, что первый пропустит: зашитую длину
/// строки или случайно захардкоженное имя.
const String _panelShapedLink =
    'caramba://connect?d=8D5320D405W1MT3MEHR76EHF5XR62VK5DGQ6AY31E9QP4VVM5ST6'
    'YW02A2FJRGDBFM746P5HRVN91WVXBCJ06TA5B10J0MJF897N818TDA9BE014BME54';

/// Срок из золотого вектора.
const int _goldenExpires = 1780000000;

Uint8List _bytes(String hex) {
  final out = Uint8List(hex.length ~/ 2);
  for (var i = 0; i < out.length; i++) {
    out[i] = int.parse(hex.substring(i * 2, i * 2 + 2), radix: 16);
  }
  return out;
}

String _hex(List<int> bytes) =>
    bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();

/// Собирает ссылку из готового конверта, ничего не пересчитывая: нужна там, где
/// проверяется именно испорченный конверт.
String _linkOf(List<int> envelope) =>
    'caramba://connect?d=${base32CrockfordEncode(envelope)}';

/// Собирает КОРРЕКТНЫЙ конверт из головы и payload, пересчитывая сумму.
///
/// Именно пересчёт делает тесты магии и версии честными: без него они прошли бы
/// на одной сумме, и было бы не видно, что отвергает разбор.
String _sealedLink({required List<int> head4, required List<int> payload}) {
  final digest = sha256(<int>[...head4, ...payload]);
  return _linkOf(<int>[...head4, ...payload, ...digest.take(4)]);
}

void main() {
  final golden = _bytes(_goldenEnvelopeHex);
  final goldenHead = golden.sublist(0, 4);
  final goldenPayload = golden.sublist(4, golden.length - 4);
  final goldenPayloadHex = _hex(goldenPayload);

  group('золотые векторы', () {
    test('ссылка панели разбирается байт в байт', () {
      // Сначала: армор из наших байт совпадает со строкой, которую выпустила
      // панель. Это и есть проверка общей раскладки, а не только своего кодека.
      expect(_linkOf(golden), _goldenLink);

      final parsed = parseConnectLink(_goldenLink, nowSec: _goldenExpires - 60);
      expect(parsed.isOk, isTrue, reason: parsed.detail);
      final link = parsed.link!;
      expect(link.origin, 'https://app.exarobot.top');
      expect(link.code, '000102030405060708090a0b0c0d0e0f');
      expect(link.operatorName, 'Caramba Connect');
      expect(link.expiresAtSec, _goldenExpires);
      // Ключа 4 в векторе нет. Отсутствие обязано читаться как отсутствие, а не
      // как пустой ключ: на живой панели церемонии ключей не было, и все
      // настоящие ссылки сейчас именно такие.
      expect(link.hasRootKeyId, isFalse);
      expect(link.rootKeyIdHex, isNull);
    });

    test('второй вектор с другим origin и именем разбирается так же', () {
      final parsed = parseConnectLink(_panelShapedLink, nowSec: 1787999999);
      expect(parsed.isOk, isTrue, reason: parsed.detail);
      expect(parsed.link!.origin, 'https://panel.exarobot.top');
      expect(parsed.link!.operatorName, 'EXA ROBOT');
      expect(parsed.link!.code, '9f2c41ab7d0e4358b1c6ea90f37d5b24');
      expect(parsed.link!.expiresAtSec, 1788000000);
    });

    test('код ссылки имеет проводную форму, которую ждёт панель', () {
      final parsed = parseConnectLink(_goldenLink, nowSec: 1);
      expect(isConnectWireCode(parsed.link!.code), isTrue);
      // Верхний регистр и другая длина проводным кодом не считаются: панель
      // отсекает такое до удара по базе тем же предикатом.
      expect(isConnectWireCode(parsed.link!.code.toUpperCase()), isFalse);
      expect(isConnectWireCode('abc'), isFalse);
    });
  });

  group('транспорт', () {
    test('нижний регистр и косметические дефисы переживают пересылку', () {
      final armor = base32CrockfordEncode(golden).toLowerCase();
      final chunked = <String>[
        armor.substring(0, 20),
        armor.substring(20, 40),
        armor.substring(40),
      ].join('-');
      final parsed = parseConnectLink(
        'caramba://connect?d=$chunked',
        nowSec: _goldenExpires - 60,
      );
      expect(parsed.isOk, isTrue, reason: parsed.detail);
      expect(parsed.link!.code, '000102030405060708090a0b0c0d0e0f');
    });

    test('действие в первом сегменте пути читается так же', () {
      final parsed = parseConnectLink(
        'caramba:///connect?d=${base32CrockfordEncode(golden)}',
        nowSec: _goldenExpires - 60,
      );
      expect(parsed.isOk, isTrue, reason: parsed.detail);
    });
  });

  group('отказы называют свою причину', () {
    test('чужая схема, чужое действие и ссылка без d', () {
      for (final raw in <String>[
        'https://app.exarobot.top/sub/feb7e480',
        'carambaconnect://enroll?panel=https://p.example&code=X',
        'caramba://import?d=8D53',
        'caramba://connect',
        'не ссылка вовсе',
      ]) {
        final parsed = parseConnectLink(raw, nowSec: 1);
        expect(
          parsed.failure,
          ConnectLinkFailure.notOurLink,
          reason: 'на входе "$raw"',
        );
      }
    });

    test('символ вне алфавита Crockford отличается от обрезанной строки', () {
      // U в алфавите нет по построению: его нельзя спутать с V при чтении.
      final withU = parseConnectLink(
        'caramba://connect?d=${base32CrockfordEncode(golden)}U',
        nowSec: 1,
      );
      expect(withU.failure, ConnectLinkFailure.armorAlphabet);

      // Обрезанный на символ армор оставляет ненулевые хвостовые биты: это уже
      // не «лишний символ», а «скопировалось не всё», и совет другой.
      final armor = base32CrockfordEncode(golden);
      final truncated = parseConnectLink(
        'caramba://connect?d=${armor.substring(0, armor.length - 1)}',
        nowSec: 1,
      );
      expect(truncated.failure, ConnectLinkFailure.armorPadding);
    });

    test('обрывок короче конверта', () {
      final parsed = parseConnectLink(
        _linkOf(<int>[0x43, 0x4a, 0x31, 0x01]),
        nowSec: 1,
      );
      expect(parsed.failure, ConnectLinkFailure.tooShort);
    });

    test('чужая магия и чужая версия отвергаются, а не сумма', () {
      // Сумма пересчитана под изменённую голову, поэтому отвергает именно
      // проверка магии и версии, а не контрольная сумма.
      final wrongMagic = _sealedLink(
        head4: <int>[0x43, 0x4a, 0x32, 0x01],
        payload: goldenPayload,
      );
      expect(
        parseConnectLink(wrongMagic, nowSec: 1).failure,
        ConnectLinkFailure.magic,
      );

      final wrongVersion = _sealedLink(
        head4: <int>[0x43, 0x4a, 0x31, 0x02],
        payload: goldenPayload,
      );
      expect(
        parseConnectLink(wrongVersion, nowSec: 1).failure,
        ConnectLinkFailure.version,
      );
    });

    test('подпорченная сумма отвергается', () {
      final tampered = Uint8List.fromList(golden);
      tampered[tampered.length - 1] ^= 0x01;
      expect(
        parseConnectLink(_linkOf(tampered), nowSec: 1).failure,
        ConnectLinkFailure.checksum,
      );
    });

    test('подпорченный байт payload ловит сумма, а не разбор CBOR', () {
      // Меняем байт внутри имени оператора: CBOR остаётся структурно
      // безупречным, и единственное, что здесь может сработать, это сумма.
      final tampered = Uint8List.fromList(golden);
      tampered[golden.length - 4 - 2] ^= 0x20;
      final parsed = parseConnectLink(_linkOf(tampered), nowSec: 1);
      expect(
        parsed.failure,
        ConnectLinkFailure.checksum,
        reason: 'сумма обязана считаться до разбора CBOR',
      );
    });

    test('карта без кода это нехватка полей, а не сбой разбора', () {
      // Убираем пару 2 => bstr(16) и уменьшаем объявленный размер карты.
      final reducedHex = goldenPayloadHex
          .replaceFirst('0250000102030405060708090a0b0c0d0e0f', '')
          .replaceFirst('a4', 'a3');
      final parsed = parseConnectLink(
        _sealedLink(head4: goldenHead, payload: _bytes(reducedHex)),
        nowSec: 1,
      );
      expect(parsed.failure, ConnectLinkFailure.fields);
      expect(parsed.detail, contains('key 2'));
    });

    test('корневой ключ неверной длины это отказ, а не «ключа нет»', () {
      // 11 байт вместо 12. Молча выкинуть такое поле значило бы показать
      // «ключа нет» там, где ключ был и приехал испорченным.
      final withShortKid = goldenPayloadHex
          .replaceFirst('a4', 'a5')
          .replaceFirst(
            '051a6a18a500',
            '044b0102030405060708090a0b051a6a18a500',
          );
      final parsed = parseConnectLink(
        _sealedLink(head4: goldenHead, payload: _bytes(withShortKid)),
        nowSec: 1,
      );
      expect(parsed.failure, ConnectLinkFailure.fields);
      expect(parsed.detail, contains('key 4'));
    });

    test('корневой ключ на 12 байт читается и показывается', () {
      final withKid = goldenPayloadHex
          .replaceFirst('a4', 'a5')
          .replaceFirst(
            '051a6a18a500',
            '044c0102030405060708090a0b0c051a6a18a500',
          );
      final parsed = parseConnectLink(
        _sealedLink(head4: goldenHead, payload: _bytes(withKid)),
        nowSec: _goldenExpires - 60,
      );
      expect(parsed.isOk, isTrue, reason: parsed.detail);
      expect(parsed.link!.hasRootKeyId, isTrue);
      expect(parsed.link!.rootKeyIdHex, '0102030405060708090a0b0c');
    });

    test('origin не по https отвергается отдельной причиной', () {
      // Тот же origin, но http. Строка становится 23 байта, а короче 24 байт
      // строгий профиль требует КРАТЧАЙШЕЙ головы: 0x77 одним байтом вместо
      // 0x78 0x17. Записанная не так, она отвергалась бы разбором CBOR, и этот
      // тест проверял бы не то, что заявляет.
      final insecure = goldenPayloadHex.replaceFirst(
        '017818'
            '68747470733a2f2f6170702e657861726f626f742e746f70',
        '0177'
            '687474703a2f2f6170702e657861726f626f742e746f70',
      );
      final parsed = parseConnectLink(
        _sealedLink(head4: goldenHead, payload: _bytes(insecure)),
        nowSec: 1,
      );
      expect(parsed.failure, ConnectLinkFailure.insecureOrigin);
    });

    test('просроченная ссылка отличается от испорченной', () {
      // Секунда в секунду уже поздно: предикат панели требует строгого «позже».
      expect(
        parseConnectLink(_goldenLink, nowSec: _goldenExpires).failure,
        ConnectLinkFailure.expired,
      );
      expect(
        parseConnectLink(_goldenLink, nowSec: _goldenExpires + 1).failure,
        ConnectLinkFailure.expired,
      );
      // nowSec == 0 означает «срок не проверяем»: разбор ссылки и решение о
      // сроке это разные вопросы, и вызывающий вправе задать только первый.
      expect(parseConnectLink(_goldenLink, nowSec: 0).isOk, isTrue);
    });

    test('у каждой причины есть свой непустой текст', () {
      final seen = <String>{};
      for (final f in ConnectLinkFailure.values) {
        expect(f.message, isNotEmpty);
        // Текст «зашифрована» здесь появиться не может: ссылка не шифруется.
        expect(f.message.toLowerCase(), isNot(contains('зашифров')));
        expect(seen.add(f.message), isTrue, reason: 'дубликат текста у $f');
      }
    });
  });

  // ---------------------------------------------------------------------------
  // Подделка экрана через имя оператора.
  //
  // Ссылку может сминтить кто угодно: формат опубликован, а хвост это
  // контрольная сумма, а не MAC. Значит, имя оператора это строка, которую
  // выбирает АТАКУЮЩИЙ, а экран подтверждения рисует её строкой «слева подпись
  // — справа значение». Перевод строки внутри имени даёт вторую такую строку,
  // выровненную ровно там же, и «Адрес панели https://app.exarobot.top» внутри
  // имени заставляет чужую панель выглядеть подлинной.
  //
  // Отсюда гейт НА ГРАНИЦЕ РАЗБОРА, а не в виджете: поле, отвергнутое здесь,
  // безопасно везде — на этом экране, в логе, на экране, которого ещё нет.
  // ---------------------------------------------------------------------------
  group('санитайзинг текстовых полей', () {
    /// Собирает payload с произвольным именем оператора: остальные поля из
    /// золотого вектора, чтобы отличалось ровно проверяемое.
    Uint8List payloadNamed(String name) {
      final nameBytes = utf8.encode(name);
      final head = <int>[];
      if (nameBytes.length < 24) {
        head.add(0x60 | nameBytes.length);
      } else if (nameBytes.length < 256) {
        head.addAll(<int>[0x78, nameBytes.length]);
      } else {
        head.addAll(<int>[
          0x79,
          (nameBytes.length >> 8) & 0xff,
          nameBytes.length & 0xff,
        ]);
      }
      return Uint8List.fromList(<int>[
        0xa4,
        ..._bytes(
          '017818'
          '68747470733a2f2f6170702e657861726f626f742e746f70',
        ),
        ..._bytes(
          '0250'
          '000102030405060708090a0b0c0d0e0f',
        ),
        0x03,
        ...head,
        ...nameBytes,
        ..._bytes('051a6a18a500'),
      ]);
    }

    ConnectLinkParse parseNamed(String name) => parseConnectLink(
      _sealedLink(head4: goldenHead, payload: payloadNamed(name)),
      nowSec: _goldenExpires - 60,
    );

    test('имя с переводом строки подделывает строку экрана и отвергается', () {
      // Ровно та строка из отчёта: вторая «строка экрана» внутри одного поля.
      final parsed = parseNamed(
        'Caramba Connect\nАдрес панели   https://app.exarobot.top',
      );
      expect(parsed.isOk, isFalse);
      expect(parsed.failure, ConnectLinkFailure.forgedText);
      expect(parsed.detail, contains('operator name'));
      expect(parsed.detail, contains('U+000A'));
      // Отвергаем, а не чистим: молча переписанное имя человек прочтёт как
      // подлинное и примет решение по строке, которой в ссылке нет.
      expect(parsed.link, isNull);
    });

    test('переносы, bidi и управляющие символы отвергаются поимённо', () {
      const hostile = <String, int>{
        'CR': 0x0d,
        'NUL': 0x00,
        'NEL': 0x85,
        'LINE SEPARATOR': 0x2028,
        'PARAGRAPH SEPARATOR': 0x2029,
        'RLO': 0x202e,
        'LRO': 0x202d,
        'RLI': 0x2067,
        'PDI': 0x2069,
        'RLM': 0x200f,
        'LRM': 0x200e,
        'ALM': 0x061c,
      };
      hostile.forEach((label, cp) {
        final parsed = parseNamed('Caramba${String.fromCharCode(cp)}Connect');
        expect(
          parsed.failure,
          ConnectLinkFailure.forgedText,
          reason: '$label (U+${cp.toRadixString(16)}) прошёл разбор',
        );
      });
    });

    test('обычное имя с юникодом и эмодзи проходит', () {
      // Гейт про управляющие символы, а не про алфавит: суррогатная пара
      // эмодзи не должна разбираться на половинки и выглядеть нарушением.
      for (final name in <String>[
        'Caramba Connect',
        'Караmba «Connect» — оператор',
        'Caramba 🛡 Connect',
        'مشغل كارامبا', // арабский БЕЗ управляющих символов направления
        '',
      ]) {
        final parsed = parseNamed(name);
        expect(parsed.isOk, isTrue, reason: '$name: ${parsed.detail}');
        expect(parsed.link!.operatorName, name);
      }
    });

    test('поле длиннее предела профиля отвергается', () {
      // 257 байт: на байт больше MAX_TSTR_BYTES. Панель теперь такое НЕ
      // выпускает (connect_link.rs, over_long_field_is_refused_at_issuance),
      // но ссылку минтит кто угодно, поэтому отказ обязан быть и здесь.
      final parsed = parseNamed('n' * (kConnectMaxFieldBytes + 1));
      expect(parsed.isOk, isFalse);
      // Декодер строгого профиля ловит это ещё в заголовке tstr — раньше, чем
      // поле дойдёт до проверки поля. Причина при этом честная: это ссылка вне
      // профиля, а не подделка экрана.
      expect(parsed.failure, ConnectLinkFailure.payload);

      // Ровно на пределе — принимается: приложение, отвергающее короче панели,
      // отвергало бы ссылки, которые панель имеет право выпустить.
      final edge = parseNamed('n' * kConnectMaxFieldBytes);
      expect(edge.isOk, isTrue, reason: edge.detail);
      expect(edge.link!.operatorName.length, kConnectMaxFieldBytes);
    });
  });

  group('looksLikeConnectLink', () {
    test('узнаёт ссылку по схеме и действию, не разбирая её', () {
      expect(looksLikeConnectLink(_goldenLink), isTrue);
      // Даже поломанную: решение «это наша ссылка» и решение «она годная» это
      // разные решения, и второе принимает экран, у которого есть место для
      // причины.
      expect(looksLikeConnectLink('caramba://connect?d=ZZZZ'), isTrue);
      expect(looksLikeConnectLink('CARAMBA://CONNECT?d=ZZZZ'), isTrue);
      expect(looksLikeConnectLink('https://app.exarobot.top/sub/x'), isFalse);
      expect(
        looksLikeConnectLink('carambaconnect://enroll?panel=x&code=y'),
        isFalse,
      );
    });
  });
}

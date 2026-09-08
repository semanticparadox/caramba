/// Разбор ссылки `caramba://connect?d=<armor>` — самоописывающегося
/// приглашения в панель.
///
/// ЗАЧЕМ. До неё единственным входом был экран «введите инвайт-код»: на живой
/// панели кодов ноль, выпускать их было нечем, а кнопка QR показывала тост.
/// Пройти этот экран не мог никто. Ссылка убирает ввод целиком: куда идти, как
/// зовут оператора и до какого момента приглашение живо, приложение читает из
/// самой ссылки, а секрет гасит одним запросом.
///
/// ЧЕСТНО О СВОЙСТВАХ. Строка НЕПРОЗРАЧНАЯ и ЦЕЛОСТНОСТНО-ПРОВЕРЯЕМАЯ, но НЕ
/// шифртекст. Зашифровать её нечем: общего секрета с оператором, которого
/// приложение ещё ни разу не видело, не существует, а адрес коннектора нужно
/// прочитать ДО любого обращения к сети. Base32 здесь транспортная броня
/// (переживает мессенджеры, регистр и переносы), а не тайна. Конфиденциальность
/// даёт способ доставки (личное сообщение бота) и то, что код одноразовый и
/// живёт полчаса. Ни в комментариях, ни в тексте для человека эту ссылку
/// нельзя называть «зашифрованной».
///
/// РАСКЛАДКА БАЙТ. Нормативный эталон — `apps/caramba-panel/src/connect_link.rs`;
/// здесь она повторена, потому что парсер обязан отвергать ровно то же самое:
///
/// ```text
/// caramba://connect?d=<armor>
///
/// <armor> = base32 Crockford (регистр любой, дефисы игнорируются, без '='):
///   0..3     "CJ1"      магия
///   3..4     0x01       версия
///   4..N-4   payload    карта CBOR строгого профиля
///   N-4..N   checksum   sha256("CJ1" || version || payload)[0..4]
///
/// payload:
///   1 => tstr  origin коннектора      https-origin панели
///   2 => bstr  code                   ровно 16 байт, одноразовый секрет
///   3 => tstr  имя оператора          для экрана подтверждения
///   4 => bstr  root key id            ровно 12 байт, ОПЦИОНАЛЬНО
///   5 => uint  expires at             unix-секунды
/// ```
///
/// Ключ 4 ОТСУТСТВУЕТ, когда оператор не проводил церемонию ключей, и на живой
/// панели сейчас именно так (таблицы `csm_keys` там нет вовсе). Отсутствие это
/// нормальное состояние, а не дефект, и отличать его от «пустой ключ» обязано
/// само приложение: поэтому поле нуллабельно, а не пустой список.
///
/// Base32 и CBOR берутся из кодека CSM (`package:caramba_vpn/csm.dart`), а не
/// пишутся заново: второй декодер это второй набор допущений о хвостовых битах
/// и канонической форме, то есть вторая поверхность для расхождения с панелью.
///
/// ССЫЛКУ МОЖЕТ СМИНТИТЬ КТО УГОДНО. Формат опубликован, а хвост это
/// контрольная сумма, а не MAC: она ловит порчу при копировании и НЕ говорит
/// ничего о том, кто строку составил. Значит, `origin` и `operator name` это
/// две РАЗНЫЕ по природе величины. Origin приложение проверит само — по нему
/// пойдёт TLS-соединение, и сертификат либо сойдётся с доменом, либо нет.
/// Имя оператора не проверяет никто и никогда: это просто текст внутри ссылки.
///
/// Отсюда санитайзинг ИМЕННО ЗДЕСЬ, на границе разбора, а не в виджете.
/// Экран рисует поля строками «слева подпись — справа значение», и перевод
/// строки внутри имени даёт вторую такую строку, визуально неотличимую от
/// настоящей: «Адрес панели   https://app.exarobot.top» внутри имени оператора
/// заставляет чужую панель выглядеть подлинной. Bidi-override делает то же
/// самое перестановкой уже отрисованного текста. Виджет починить можно (и он
/// починен — `maxLines: 1`), но починка виджета защищает ровно один виджет;
/// поле, отвергнутое разбором, безопасно везде, включая логи и будущие экраны.
///
/// Отвергаем, а не чистим. Молча переписанное имя выглядело бы как подлинное
/// имя оператора, и человек принял бы решение по строке, которой в ссылке нет.
library;

import 'dart:convert' show utf8;
import 'dart:typed_data';

import 'package:caramba_vpn/csm.dart'
    show
        CborBytes,
        CborMap,
        CborText,
        CborUint,
        CsmError,
        base32CrockfordDecode,
        csmCrockfordAlphabet,
        csmDecodePayload,
        sha256;

import 'package:caramba_client/data/models/csm_enrollment.dart'
    show csmNormalizeOrigin;

/// Схема и действие ссылки.
const String kConnectLinkScheme = 'caramba';
const String kConnectLinkAction = 'connect';

/// Магия конверта, ASCII «CJ1».
const List<int> kConnectLinkMagic = [0x43, 0x4a, 0x31];

/// Версия конверта, которую понимает эта сборка.
const int kConnectLinkVersion = 0x01;

/// Длина одноразового секрета в байтах.
const int kConnectCodeBytes = 16;

/// Длина идентификатора корневого ключа в байтах.
const int kConnectRootKidBytes = 12;

/// Длина хвостовой контрольной суммы.
const int kConnectChecksumBytes = 4;

/// Предел длины текстового поля ссылки в БАЙТАХ UTF-8.
///
/// Ровно `MAX_TSTR_BYTES` строгого профиля (`csmMaxTstrBytes`, он же
/// `cbor::MAX_TSTR_BYTES` на панели) — и это не совпадение, а требование:
/// приложение, отвергающее короче панели, отвергало бы ссылки, которые панель
/// имеет право выпустить, а панель, выпускающая длиннее, выдавала бы ссылки,
/// которые приложение обязано отвергнуть. Обе половины гейта живут по одному
/// числу.
///
/// Проверка здесь ДУБЛИРУЕТ предел декодера CBOR (тот отвергает такой tstr
/// раньше, ещё в заголовке) намеренно: там это предел кодека, здесь — предел
/// поля, которое рисуется человеку. Если профиль когда-нибудь поднимет
/// MAX_TSTR_BYTES ради чужого документа, эта строка останется на своём месте.
const int kConnectMaxFieldBytes = 256;

/// Ключи карты payload.
const int _kOrigin = 1;
const int _kCode = 2;
const int _kOperator = 3;
const int _kRootKid = 4;
const int _kExpires = 5;

/// Почему ссылка отвергнута.
///
/// Разделено детально не ради полноты: пользователь, которому сказали
/// «ссылка не подошла», введёт ту же ссылку ещё раз. Обрезанная при копировании
/// строка, ссылка от другой версии приложения и просто просроченное
/// приглашение требуют трёх разных действий человека, поэтому и три разных
/// текста.
enum ConnectLinkFailure {
  /// Это не `caramba://connect?d=...`: чужая схема, другое действие или нет `d`.
  notOurLink,

  /// В armor есть символ вне алфавита Crockford.
  armorAlphabet,

  /// Хвостовые биты armor ненулевые: строку обрезали или дописали.
  armorPadding,

  /// Байт меньше, чем минимальный конверт.
  tooShort,

  /// Первые три байта не «CJ1».
  magic,

  /// Версия конверта не та, которую понимает эта сборка.
  version,

  /// Контрольная сумма не сошлась: байты испорчены.
  checksum,

  /// Payload не разбирается строгим профилем CBOR.
  payload,

  /// Карта разобралась, но полей не хватает или они не той формы/длины.
  fields,

  /// Текстовое поле несёт символы, которыми подделывают вид экрана: перевод
  /// строки, разделитель абзаца, управление направлением письма. Ссылка
  /// структурно цела — отвергается именно попытка подделки.
  forgedText,

  /// Origin коннектора не https и не `.onion` (INV-8).
  insecureOrigin,

  /// Приглашение просрочено. Структурно ссылка целая.
  expired,
}

/// Разобранное приглашение.
class CarambaConnectLink {
  const CarambaConnectLink({
    required this.origin,
    required this.code,
    required this.operatorName,
    required this.expiresAtSec,
    this.rootKeyId,
  });

  /// Origin коннектора, нормализованный до `https://host[:port]`. Именно сюда
  /// приложение пойдёт за сессией; это НЕ обязательно домен подписки.
  final String origin;

  /// Одноразовый секрет в проводной форме: 32 hex-символа нижнего регистра.
  /// Сырые байты по сети не ходят — панель принимает и хранит именно hex.
  final String code;

  /// Имя оператора, как его ЗАЯВЛЯЕТ ссылка. Целостность байт проверена
  /// контрольной суммой, но подписи под ними нет, поэтому это утверждение
  /// отправителя, а не доказанный факт. Экран подтверждения обязан показывать
  /// его рядом с origin, а не вместо.
  final String operatorName;

  /// Идентификатор корневого ключа, 12 байт, если оператор проводил церемонию.
  /// `null` означает, что ключа в ссылке НЕТ (поле отсутствовало) — обычное
  /// состояние панели без включённого CSM, а не ошибка.
  final Uint8List? rootKeyId;

  /// Момент истечения приглашения, unix-секунды.
  final int expiresAtSec;

  /// Есть ли в ссылке идентификатор корневого ключа.
  bool get hasRootKeyId => rootKeyId != null;

  /// Hex идентификатора корневого ключа или `null`, когда его нет.
  String? get rootKeyIdHex {
    final kid = rootKeyId;
    return kid == null ? null : _hex(kid);
  }

  /// Сколько осталось до истечения относительно [nowSec]. Отрицательное
  /// значение означает, что приглашение уже просрочено.
  Duration remaining(int nowSec) => Duration(seconds: expiresAtSec - nowSec);
}

/// Результат разбора: либо ссылка, либо причина отказа с деталью для лога.
class ConnectLinkParse {
  const ConnectLinkParse.ok(CarambaConnectLink this.link)
    : failure = null,
      detail = null;

  const ConnectLinkParse.refused(ConnectLinkFailure this.failure, this.detail)
    : link = null;

  final CarambaConnectLink? link;
  final ConnectLinkFailure? failure;

  /// Техническая деталь (что именно не сошлось). Для лога и для отчёта в
  /// поддержку, не для основного текста экрана.
  final String? detail;

  bool get isOk => link != null;
}

/// Текст отказа для человека. Каждая причина называет СВОЁ действие: сказать
/// «проверьте ссылку» на просроченное приглашение значит отправить человека
/// вставлять ту же строку по кругу.
extension ConnectLinkFailureMessage on ConnectLinkFailure {
  String get message => switch (this) {
    ConnectLinkFailure.notOurLink =>
      'Это не ссылка подключения Caramba. Нужна строка, которая начинается '
          'с caramba://connect.',
    ConnectLinkFailure.armorAlphabet =>
      'В ссылке есть посторонние символы. Скорее всего скопировалась не вся '
          'строка или вместе с ней попал текст сообщения.',
    ConnectLinkFailure.armorPadding =>
      'Ссылка скопирована не полностью: конец строки потерян. Скопируйте её '
          'целиком и вставьте снова.',
    ConnectLinkFailure.tooShort =>
      'Ссылка слишком короткая: это обрывок, а не приглашение целиком.',
    ConnectLinkFailure.magic =>
      'Это ссылка другого формата: приложение её не понимает.',
    ConnectLinkFailure.version =>
      'Ссылку выпустила более новая панель. Обновите приложение.',
    ConnectLinkFailure.checksum =>
      'Ссылка повреждена: содержимое не сходится с её контрольной суммой. '
          'Попросите оператора прислать новую.',
    ConnectLinkFailure.payload =>
      'Содержимое ссылки не разбирается. Попросите оператора прислать новую.',
    ConnectLinkFailure.fields =>
      'В ссылке не хватает данных о панели. Попросите оператора прислать новую.',
    ConnectLinkFailure.forgedText =>
      'Ссылка отклонена: имя оператора в ней составлено так, чтобы подделать '
          'вид этого экрана. Ссылку выпустила не панель, которой вы доверяете. '
          'Не открывайте её и возьмите ссылку у оператора напрямую.',
    ConnectLinkFailure.insecureOrigin =>
      'Адрес панели в ссылке не по https. По открытому каналу подключаться '
          'нельзя: попросите оператора прислать https-ссылку.',
    ConnectLinkFailure.expired =>
      'Приглашение просрочено. Запросите новую ссылку у бота панели: она '
          'действует 30 минут.',
  };
}

/// Похоже ли это на ссылку подключения. Дешёвая проверка ДО разбора: нужна
/// там, где приложение решает, наша это вставленная строка или подписка.
bool looksLikeConnectLink(String raw) {
  final uri = Uri.tryParse(raw.trim());
  if (uri == null) return false;
  if (uri.scheme.toLowerCase() != kConnectLinkScheme) return false;
  return _action(uri) == kConnectLinkAction;
}

/// Проводная форма кода: ровно 32 hex-символа нижнего регистра.
///
/// Тот же предикат, что `connect_link::is_wire_code` на панели. Он же
/// отличает приглашение устройства от реферального enroll-кода: по нему
/// панель выбирает пространство имён `lnk_<hex>` в таблице кодов.
bool isConnectWireCode(String raw) {
  if (raw.length != kConnectCodeBytes * 2) return false;
  for (final u in raw.codeUnits) {
    final digit = u >= 0x30 && u <= 0x39;
    final lower = u >= 0x61 && u <= 0x66;
    if (!digit && !lower) return false;
  }
  return true;
}

/// Разбирает ссылку. [nowSec] задаётся явно, чтобы проверка срока была
/// детерминированной в тестах и чтобы вызывающий мог разобрать ссылку, не
/// проверяя срок вовсе (передав 0).
///
/// Порядок проверок нормативен: контрольная сумма проверяется ДО разбора CBOR.
/// Иначе испорченный байт payload всплывал бы как «не разбирается CBOR», то
/// есть как ошибка формата вместо ошибки передачи, и человек получил бы совет
/// просить новую ссылку там, где достаточно скопировать ту же целиком.
ConnectLinkParse parseConnectLink(String raw, {required int nowSec}) {
  final uri = Uri.tryParse(raw.trim());
  if (uri == null || uri.scheme.toLowerCase() != kConnectLinkScheme) {
    return const ConnectLinkParse.refused(
      ConnectLinkFailure.notOurLink,
      'scheme is not caramba://',
    );
  }
  if (_action(uri) != kConnectLinkAction) {
    return const ConnectLinkParse.refused(
      ConnectLinkFailure.notOurLink,
      'action is not connect',
    );
  }
  final armor = (uri.queryParameters['d'] ?? '').trim();
  if (armor.isEmpty) {
    return const ConnectLinkParse.refused(
      ConnectLinkFailure.notOurLink,
      'query parameter d is missing',
    );
  }

  final bytes = base32CrockfordDecode(armor);
  if (bytes == null) {
    // Декодер сообщает один null на две разные беды. Различаем их сами:
    // посторонний символ означает «скопировалось лишнее», ненулевой хвост —
    // «скопировалось не всё», и это разные советы человеку.
    return _armorFailure(armor);
  }

  const minimum =
      3 /* magic */ + 1 /* version */ + 1 /* payload */ + kConnectChecksumBytes;
  if (bytes.length < minimum) {
    return ConnectLinkParse.refused(
      ConnectLinkFailure.tooShort,
      'envelope is ${bytes.length} bytes, minimum is $minimum',
    );
  }
  for (var i = 0; i < kConnectLinkMagic.length; i++) {
    if (bytes[i] != kConnectLinkMagic[i]) {
      return const ConnectLinkParse.refused(
        ConnectLinkFailure.magic,
        'envelope does not start with CJ1',
      );
    }
  }
  if (bytes[3] != kConnectLinkVersion) {
    return ConnectLinkParse.refused(
      ConnectLinkFailure.version,
      'envelope version is ${bytes[3]}, this build understands '
      '$kConnectLinkVersion',
    );
  }

  final payload = Uint8List.sublistView(
    bytes,
    4,
    bytes.length - kConnectChecksumBytes,
  );
  final signedPrefix = Uint8List.sublistView(bytes, 0, 4);
  final digest = sha256(<int>[...signedPrefix, ...payload]);
  for (var i = 0; i < kConnectChecksumBytes; i++) {
    if (digest[i] != bytes[bytes.length - kConnectChecksumBytes + i]) {
      return const ConnectLinkParse.refused(
        ConnectLinkFailure.checksum,
        'sha256("CJ1" || version || payload)[0..4] does not match the tail',
      );
    }
  }

  final CborMap map;
  try {
    map = csmDecodePayload(Uint8List.fromList(payload));
  } on CsmError catch (e) {
    return ConnectLinkParse.refused(ConnectLinkFailure.payload, e.detail);
  }

  final originValue = map[_kOrigin];
  final codeValue = map[_kCode];
  final operatorValue = map[_kOperator];
  final expiresValue = map[_kExpires];
  if (originValue is! CborText) {
    return const ConnectLinkParse.refused(
      ConnectLinkFailure.fields,
      'key 1 (connector origin) is missing or not a text string',
    );
  }
  if (codeValue is! CborBytes || codeValue.value.length != kConnectCodeBytes) {
    return const ConnectLinkParse.refused(
      ConnectLinkFailure.fields,
      'key 2 (code) is missing or is not exactly 16 bytes',
    );
  }
  if (operatorValue is! CborText) {
    return const ConnectLinkParse.refused(
      ConnectLinkFailure.fields,
      'key 3 (operator name) is missing or not a text string',
    );
  }
  if (expiresValue is! CborUint) {
    return const ConnectLinkParse.refused(
      ConnectLinkFailure.fields,
      'key 5 (expires at) is missing or not an unsigned integer',
    );
  }

  // Санитайзинг текстовых полей — ДО того, как из них что-либо соберётся.
  // Отказ, а не чистка: см. шапку. Порядок «сначала подделка, потом длина»
  // намеренный — про строку с переводами строк человеку нужно сказать про
  // подделку, а не про размер.
  final originText = originValue.value;
  final operatorText = operatorValue.value;
  for (final field in <(String, String)>[
    ('connector origin', originText),
    ('operator name', operatorText),
  ]) {
    final cp = _forgingCodePoint(field.$2);
    if (cp != null) {
      return ConnectLinkParse.refused(
        ConnectLinkFailure.forgedText,
        '${field.$1} contains U+${cp.toRadixString(16).toUpperCase().padLeft(4, '0')}, '
        'which can forge a row on the confirmation screen',
      );
    }
    final bytes = utf8.encode(field.$2).length;
    if (bytes > kConnectMaxFieldBytes) {
      return ConnectLinkParse.refused(
        ConnectLinkFailure.fields,
        '${field.$1} is $bytes bytes, the strict profile allows '
        '$kConnectMaxFieldBytes',
      );
    }
  }

  // Ключ 4 опционален, но если он ЕСТЬ, он обязан быть ровно 12 байт: 11 или
  // 13 это не «панель без церемонии», а испорченная ссылка, и молча выкинуть
  // такое поле значило бы показать «ключа нет» там, где он был.
  Uint8List? rootKeyId;
  if (map.has(_kRootKid)) {
    final kid = map[_kRootKid];
    if (kid is! CborBytes || kid.value.length != kConnectRootKidBytes) {
      return const ConnectLinkParse.refused(
        ConnectLinkFailure.fields,
        'key 4 (root key id) is present but is not exactly 12 bytes',
      );
    }
    rootKeyId = kid.value;
  }

  final origin = csmNormalizeOrigin(originText);
  if (origin == null) {
    return ConnectLinkParse.refused(
      ConnectLinkFailure.insecureOrigin,
      'connector origin "$originText" is not https and not .onion',
    );
  }

  final link = CarambaConnectLink(
    origin: origin,
    code: _hex(codeValue.value),
    operatorName: operatorText,
    rootKeyId: rootKeyId,
    expiresAtSec: expiresValue.value,
  );

  // Срок проверяется ПОСЛЕДНИМ и отдельной причиной: ссылка целая, всё
  // прочитано, погасить её просто уже нельзя. Человеку нужно попросить новую,
  // а не чинить эту.
  if (nowSec > 0 && link.expiresAtSec <= nowSec) {
    return ConnectLinkParse.refused(
      ConnectLinkFailure.expired,
      'invite expired at ${link.expiresAtSec}, now is $nowSec',
    );
  }
  return ConnectLinkParse.ok(link);
}

/// Первая кодовая точка, которой можно подделать вид экрана, или `null`.
///
/// Список нормативен и повторён в `connect_link.rs` (`check_printable`) —
/// запрещать в приложении шире, чем на панели, значит отвергать законные
/// ссылки; уже, чем на панели, — пропускать подделки.
///
///   * C0 (00..1F) и DEL/C1 (7F..9F) — сюда попадают `\n` и `\r`: именно они
///     превращают одно поле в несколько строк экрана;
///   * U+2028/U+2029 — те же переносы «по-юникодному», Flutter ломает строку
///     и по ним;
///   * U+200E/U+200F/U+061C, U+202A..U+202E, U+2066..U+2069 — управление
///     направлением письма: переставляет уже отрисованный текст, из-за чего
///     видимая строка перестаёт соответствовать байтам.
///
/// Идёт по РУНАМ, а не по code units: суррогатная пара не должна разбираться
/// на половинки, иначе эмодзи в имени оператора выглядели бы нарушением.
int? _forgingCodePoint(String s) {
  for (final cp in s.runes) {
    final hostile =
        cp <= 0x1f ||
        (cp >= 0x7f && cp <= 0x9f) ||
        cp == 0x2028 ||
        cp == 0x2029 ||
        cp == 0x200e ||
        cp == 0x200f ||
        cp == 0x061c ||
        (cp >= 0x202a && cp <= 0x202e) ||
        (cp >= 0x2066 && cp <= 0x2069);
    if (hostile) return cp;
  }
  return null;
}

/// Различает две беды, которые декодер base32 сообщает одинаковым `null`.
ConnectLinkParse _armorFailure(String armor) {
  for (final u in armor.codeUnits) {
    if (u == 0x2d) continue; // дефис косметический, декодер его игнорирует
    final ch = String.fromCharCode(u).toUpperCase();
    final folded = switch (ch) {
      'I' || 'L' => '1',
      'O' => '0',
      _ => ch,
    };
    if (!csmCrockfordAlphabet.contains(folded)) {
      return ConnectLinkParse.refused(
        ConnectLinkFailure.armorAlphabet,
        'character "$ch" is outside the Crockford alphabet',
      );
    }
  }
  return const ConnectLinkParse.refused(
    ConnectLinkFailure.armorPadding,
    'trailing padding bits are not zero',
  );
}

/// Действие custom-scheme URI: хост либо первый сегмент пути, нижним регистром.
/// `caramba://connect` и `caramba:///connect` это одно и то же действие.
String _action(Uri uri) {
  final raw = uri.host.isNotEmpty
      ? uri.host
      : (uri.pathSegments.isNotEmpty ? uri.pathSegments.first : '');
  return raw.toLowerCase();
}

String _hex(List<int> bytes) {
  final sb = StringBuffer();
  for (final b in bytes) {
    sb.write(b.toRadixString(16).padLeft(2, '0'));
  }
  return sb.toString();
}

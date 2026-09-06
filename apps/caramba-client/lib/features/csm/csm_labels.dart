/// Русские подписи для экранов проверки CSM/1 и разбор кодов отказа.
///
/// Нормативно: 02-SPEC.md 8.8 (что пользователь обязан видеть), 8.8.1 (класс
/// строки на каждый код из реестра 03-WIRE.md 6.6), 8.8.2 (три строки хрома,
/// которых требует модель угроз), 03-WIRE.md 1.2 (реестр типов документов).
///
/// Один файл на весь словарь намеренно: 8.8.1 существует ровно потому, что три
/// команды, читающие один корпус, иначе нарисуют для одной фикстуры три разных
/// вещи. Внутри одного приложения то же самое делают три экрана.
library;

import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';

/// Класс строки, которую видит пользователь (02-SPEC.md 8.8.1). Различие между
/// ними это единственное, на что пользователь может отреагировать.
enum CsmStringClass {
  /// Что-то в пути вернуло не наши байты. Рендерится как отказ ступени в
  /// истории попыток и НИКОГДА как утверждение о безопасности.
  transport,

  /// Мы держим документ, который сейчас нечем заменить.
  stale,

  /// Документ пришёл и не проверяется против ключей этого оператора.
  authenticity,

  /// Профиль не может продолжать на тех данных, что держит.
  fatal,
}

/// Подпись класса строки для хрома проверки.
String csmStringClassLabel(CsmStringClass c) => switch (c) {
  CsmStringClass.transport => 'Транспорт',
  CsmStringClass.stale => 'Устаревание',
  CsmStringClass.authenticity => 'Подлинность',
  CsmStringClass.fatal => 'Фатально',
};

/// Класс строки по коду отказа. Разбор и проверка это РАЗНЫЕ исходы, поэтому
/// коды семейства `E_PARSE_*` никогда не читаются как утверждение о подписи.
///
/// [beforeEnrollment] нужен ровно одному коду: `E_VERIFY_NOANCHOR` до
/// энроллмента это не отказ, а состояние «якоря ещё нет».
CsmStringClass csmStringClassOf(String code, {bool beforeEnrollment = false}) {
  if (code.startsWith('E_PARSE_')) {
    return CsmStringClass.transport;
  }
  switch (code) {
    case 'E_VERIFY_NOANCHOR':
      return beforeEnrollment
          ? CsmStringClass.transport
          : CsmStringClass.authenticity;
    case 'E_VERIFY_PID':
    case 'E_VERIFY_NONCE':
    case 'E_VERIFY_DEVICE':
    case 'E_VERIFY_CATHASH':
    case 'E_SEAL_RECIPIENT':
      return CsmStringClass.transport;
    case 'E_VERIFY_IAT':
    case 'E_VERIFY_EXPIRED':
      return CsmStringClass.stale;
    case 'E_VERIFY_ROLE':
    case 'E_VERIFY_UNAUTHORIZED':
    case 'E_VERIFY_THRESHOLD':
    case 'E_VERIFY_SIG':
    case 'E_VERIFY_REVOKED':
    case 'E_VERIFY_VERSION':
    case 'E_VERIFY_ROTATION':
    case 'E_SEAL_SUITE':
    case 'E_SEAL_OPEN':
      return CsmStringClass.authenticity;
    default:
      return CsmStringClass.fatal;
  }
}

/// Что код значит человеку. Текст описывает НАБЛЮДЕНИЕ, а не вердикт о
/// злонамеренности: обрыв в пути и подделка подписи выглядят по-разному, и
/// путать их нельзя (02-SPEC.md 8.8.1).
String csmErrorText(String code) => switch (code) {
  'ok' => 'Проверено',
  'E_PARSE_SHORT' => 'Ответ короче кадра',
  'E_PARSE_MAGIC' => 'Ответ не начинается сигнатурой CSM1',
  'E_PARSE_DOCTYPE' => 'Неизвестный тип документа',
  'E_PARSE_LEN' => 'Длина полезной нагрузки не сходится',
  'E_PARSE_NSIGS' => 'Число подписей вне допустимого',
  'E_PARSE_FRAMING' => 'Длина кадра не точная: есть лишние байты',
  'E_PARSE_SLOTORDER' => 'Слоты подписей идут не по порядку',
  'E_PARSE_CBOR' => 'CBOR нарушает строгий профиль разбора',
  'E_PARSE_ENVELOPE' => 'Общий конверт документа неполон',
  'E_PARSE_FIELD' => 'Поле документа не той формы',
  'E_VERIFY_ROLE' => 'Ключ не имеет роли для этого типа документа',
  'E_VERIFY_NOANCHOR' => 'Нет доверенного ключевого документа',
  'E_VERIFY_UNAUTHORIZED' => 'Подписавший не в наборе ключей этой роли',
  'E_VERIFY_REVOKED' => 'Ключ подписавшего отозван',
  'E_VERIFY_SIG' => 'Подпись не сошлась',
  'E_VERIFY_THRESHOLD' => 'Подписей меньше порога роли',
  'E_VERIFY_PID' => 'Документ подписан другим оператором',
  'E_VERIFY_VERSION' => 'Версия не выше уже принятой',
  'E_VERIFY_ROTATION' => 'Ротация корня нарушает правило версии N+1',
  'E_VERIFY_IAT' => 'Дата выпуска ниже временного пола',
  'E_VERIFY_EXPIRED' => 'Документ просрочен',
  'E_VERIFY_NONCE' => 'Одноразовое число не совпало с отправленным',
  'E_VERIFY_DEVICE' => 'Директива выписана не этому устройству',
  'E_VERIFY_CATHASH' => 'Хеш каталога не совпал с названным',
  'E_SEAL_RECIPIENT' => 'Запечатано не на ключ этого устройства',
  'E_SEAL_SUITE' => 'Неизвестный набор шифров запечатывания',
  'E_SEAL_OPEN' => 'Запечатанный конверт не открывается',
  _ => code,
};

/// Название типа документа (03-WIRE.md 1.2).
String csmDocTypeName(int docType) => switch (docType) {
  0x01 => 'Ключевой документ',
  0x02 => 'Каталог',
  0x03 => 'Директива',
  0x04 => 'Часть каталога',
  0x05 => 'Бутстрап-блоб',
  0x06 => 'Запечатанная директива',
  0x08 => 'Резервный пул',
  _ => 'Документ 0x${docType.toRadixString(16).padLeft(2, '0')}',
};

/// Короткое имя маршрута документа (`k1`, `c1`, `m1`).
String csmDocShortName(int docType) => switch (docType) {
  0x01 => 'k1',
  0x02 => 'c1',
  0x03 => 'm1',
  0x04 => 'c1c',
  0x05 => 'b1',
  0x06 => 'm1s',
  0x08 => 'r1',
  _ => '0x${docType.toRadixString(16).padLeft(2, '0')}',
};

/// Роль, подписавшая документ (03-WIRE.md 7.1). Роль читается из ранее
/// доверенного документа, а не из проверяемого; здесь только подпись для глаз.
String csmDocRoleName(int docType) => switch (docType) {
  0x01 || 0x05 || 0x08 => 'корневой ключ',
  0x02 || 0x03 || 0x04 || 0x06 => 'онлайновый ключ',
  _ => 'неизвестная роль',
};

/// Ступень лестницы: короткий идентификатор `R0`..`R6`.
String csmRungId(CsmRung rung) => 'R${rung.id}';

/// Ступень лестницы по-русски (02-SPEC.md 8.1).
String csmRungTitle(CsmRung rung) => switch (rung) {
  CsmRung.cached => 'Сохранённые документы',
  CsmRung.direct => 'Прямой HTTPS к оператору',
  CsmRung.mirrors => 'Подписанные зеркала',
  CsmRung.doh => 'Адрес через DoH с явным SNI',
  CsmRung.tunnel => 'Через собственный туннель',
  CsmRung.userProxy => 'Свой прокси SOCKS5 или HTTP',
  CsmRung.outOfBand => 'Вне полосы: файл, QR, диктовка',
};

/// Что ступень делает, одной строкой.
String csmRungDesc(CsmRung rung) => switch (rung) {
  CsmRung.cached =>
    'Проверенные кадры с диска. Работает без сети и отключить её нельзя.',
  CsmRung.direct => 'Запрос на тот origin, где закреплён корневой ключ.',
  CsmRung.mirrors => 'Пул зеркал из подписанного каталога, по очереди.',
  CsmRung.doh => 'Имя резолвится по DoH, соединение идёт с явным SNI.',
  CsmRung.tunnel => 'Запрос уходит внутрь уже поднятого туннеля.',
  CsmRung.userProxy => 'Прокси, который вы ввели сами.',
  CsmRung.outOfBand =>
    'Кадр, принесённый человеком. Отключить её нельзя никогда.',
};

/// Почему ступень или контрол недоступны. Словарь закрытый (02-SPEC.md 8.1).
String csmUnavailableReasonText(
  CsmUnavailableReason reason,
) => switch (reason) {
  CsmUnavailableReason.userDisabled => 'Выключена вами',
  CsmUnavailableReason.notOfferedByOperator => 'Оператор её не предлагает',
  CsmUnavailableReason.platformUnsupported => 'Платформа этого не поддерживает',
  CsmUnavailableReason.notConfigured => 'Не настроена',
  CsmUnavailableReason.appVersionUnsupported => 'Версия приложения её не знает',
};

/// Состояние профиля (02-SPEC.md 2.1) по-русски.
String csmStageTitle(CsmProfileStage stage) => switch (stage) {
  CsmProfileStage.unenrolled => 'Не подключён',
  CsmProfileStage.pinning => 'Закрепляем корневой ключ',
  CsmProfileStage.anchored => 'Корневой ключ закреплён',
  CsmProfileStage.enrolled => 'Устройство зарегистрировано',
  CsmProfileStage.trusted => 'Проверено',
  CsmProfileStage.trustedStale => 'Проверено, конфигурация не обновлялась',
  CsmProfileStage.grace => 'Работает на сохранённой конфигурации',
  CsmProfileStage.graceExhausted => 'Окно офлайн-работы исчерпано',
  CsmProfileStage.compromised => 'Доверие отозвано',
};

/// Как был установлен пин (INV-18). Разница между этими двумя и есть то, что
/// пользователь обязан увидеть: закреплённый по ссылке пин пришёл с того же
/// origin, что и документы, и переживает захват канала оператора хуже.
String csmPinOriginTitle(CsmPinOrigin origin) => switch (origin) {
  CsmPinOrigin.outOfBand => 'Вне полосы',
  CsmPinOrigin.inApp => 'В приложении',
};

String csmPinOriginDesc(CsmPinOrigin origin) => switch (origin) {
  CsmPinOrigin.outOfBand =>
    'Отпечаток продиктован отдельно от канала оператора: QR, бумага, голос. '
        'Такой пин переживает захват канала.',
  CsmPinOrigin.inApp =>
    'Отпечаток пришёл ссылкой энроллмента с того же адреса, что и документы. '
        'Это слабее: захвативший адрес мог прислать и ссылку.',
};

/// Уровень хранения ключа устройства (02-SPEC.md 9.4).
String csmHardwareTierTitle(CsmHardwareTier tier) => switch (tier) {
  CsmHardwareTier.secureEnclave => 'Secure Enclave',
  CsmHardwareTier.teeOrStrongbox => 'TEE или StrongBox',
  CsmHardwareTier.software => 'Программное хранилище',
};

/// Настройка по-русски. Имена совпадают с подписями на экранах настроек, чтобы
/// карточка «Оставить или Вернуть» называла ту же строку, что видит человек.
String csmSettingTitle(CsmSettingKey key) => switch (key) {
  CsmSettingKey.protocol => 'Тип подключения',
  CsmSettingKey.preset => 'Режим',
  CsmSettingKey.relay => 'Relay (вход)',
  CsmSettingKey.stack => 'Сетевой стек (TUN)',
  CsmSettingKey.mtu => 'MTU',
  CsmSettingKey.ipv6 => 'IPv6',
  CsmSettingKey.fakeIp => 'Fake-IP',
  CsmSettingKey.killSwitch => 'Kill-switch',
  CsmSettingKey.dnsNameservers => 'DNS-резолверы',
  CsmSettingKey.dnsFallback => 'Запасные резолверы',
  CsmSettingKey.splitMode => 'Правила по сайтам',
};

/// Происхождение значения (02-SPEC.md 7.6). Карточка обязана его называть.
String csmProvenanceTitle(CsmProvenance src) => switch (src) {
  CsmProvenance.user => 'вы',
  CsmProvenance.operator => 'оператор',
  CsmProvenance.byDefault => 'значение по умолчанию',
};

/// Значение настройки для показа. Пустая строка у `relay` это ТРЕТЬЕ
/// состояние, «не выбрано», а не «выключено» (02-SPEC.md 7.3, Correction 15).
String csmSettingValueText(CsmSettingKey key, CsmSettingValue value) {
  if (value is CsmText) {
    final v = value.value;
    if (key == CsmSettingKey.relay) {
      if (v.isEmpty) return 'не выбрано';
      if (v == kCsmNoRelay) return 'без релея';
      return v;
    }
    if (v.isEmpty) return 'не выбрано';
    if (v == 'auto') return 'авто';
    return v;
  }
  if (value is CsmUint) {
    return value.value == 0 ? 'по умолчанию ядра' : '${value.value}';
  }
  if (value is CsmBoolean) {
    return value.value ? 'включено' : 'выключено';
  }
  if (value is CsmTextList) {
    return value.value.isEmpty ? 'пусто' : value.value.join(', ');
  }
  return '$value';
}

/// Возможность оператора по-русски (03-WIRE.md 5.1).
String csmCapabilityTitle(CsmCapability c) => switch (c) {
  CsmCapability.perNodeMaterial => 'Материал узлов в каталоге',
  CsmCapability.sealedDirectives => 'Запечатанные директивы',
  CsmCapability.relayChaining => 'Цепочки релеев',
  CsmCapability.settingsWrite => 'Запись настроек',
  CsmCapability.mirrorPool => 'Пул зеркал',
  CsmCapability.dohEndpoints => 'Точки DoH',
  CsmCapability.resourceHashes => 'Хеши правил и geo',
  CsmCapability.deprecationChannel => 'Канал устареваний',
  CsmCapability.onboardingGrant => 'Онбординг-грант трафика',
  CsmCapability.deviceEnrollment => 'Регистрация второго устройства',
  CsmCapability.variantForwarding => 'Проброс варианта',
  CsmCapability.portHopping => 'Прыжки по портам',
};

/// Дата и время по-русски, без секунд. Нулевая метка означает «неизвестно».
String csmDateTime(int epochMs) {
  if (epochMs <= 0) return 'неизвестно';
  final d = DateTime.fromMillisecondsSinceEpoch(epochMs).toLocal();
  String two(int v) => v.toString().padLeft(2, '0');
  return '${two(d.day)}.${two(d.month)}.${d.year} ${two(d.hour)}:${two(d.minute)}';
}

/// То же для секундной метки провода.
String csmDateTimeSec(int epochSec) => csmDateTime(epochSec * 1000);

/// Возраст в человеческом виде: минуты, часы, дни.
String csmAgeText(int seconds) {
  if (seconds < 0) return '0 минут';
  if (seconds < 60) return '$seconds с';
  final minutes = seconds ~/ 60;
  if (minutes < 60) {
    return '$minutes ${_plural(minutes, 'минута', 'минуты', 'минут')}';
  }
  final hours = minutes ~/ 60;
  if (hours < 48) return '$hours ${_plural(hours, 'час', 'часа', 'часов')}';
  final days = hours ~/ 24;
  return '$days ${_plural(days, 'день', 'дня', 'дней')}';
}

String _plural(int n, String one, String few, String many) {
  final mod100 = n % 100;
  if (mod100 >= 11 && mod100 <= 14) return many;
  switch (n % 10) {
    case 1:
      return one;
    case 2:
    case 3:
    case 4:
      return few;
    default:
      return many;
  }
}

/// Инертный текст оператора для показа (INV-10, 03-WIRE.md 14.6).
///
/// Ограничивается 80 символами, из него вырезается всё, что читается как
/// ссылка, и управляющие символы. Приложение НЕ открывает ни одной ссылки,
/// пришедшей от оператора, поэтому и показывать её как ссылку нельзя: строка,
/// которую нельзя нажать, но которая выглядит нажимаемой, это приглашение
/// перепечатать её в браузер руками.
String csmInertText(String raw, {int cap = 80}) {
  var s = raw.replaceAll(RegExp(r'[\x00-\x1f\x7f]'), ' ');
  // Схемы и голые домены уносятся целиком, а не «обезвреживаются» точками.
  //
  // Правило ПОЛОЖИТЕЛЬНОЕ: список доменов первого уровня выразить требуемое не
  // может, потому что evil.co, t.ly, bit.do, example.shop и голый IPv4 с путём
  // его переживают, а INV-10 говорит про «похожее на ссылку», а не про
  // «оканчивающееся на .com». Выносится всё, что несёт схему, собаку, или
  // выглядит как host.tld с необязательным путём или портом.
  s = s.replaceAll(
    RegExp(
      // что угодно со схемой
      r'(?:[a-zA-Z][a-zA-Z0-9+.-]*:\/\/\S*)'
      // user@host
      r'|(?:\S*@\S+)'
      // host.tld[:port][/path], включая голый IPv4
      r'|(?:\b(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?\.)+'
      r'[a-zA-Z0-9-]{2,}(?::\d{1,5})?(?:\/\S*)?)',
    ),
    '',
  );
  s = s.replaceAll(RegExp(r'\s+'), ' ').trim();
  if (s.length > cap) {
    s = '${s.substring(0, cap)}...';
  }
  return s;
}

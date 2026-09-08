/// Энроллмент CSM/1: код, бутстрап-блоб и установка пина.
///
/// Нормативно: 02-SPEC.md 9 целиком (код, выпуск, ключи устройства, поток,
/// блоб, диплинки, режимы отказа), 03-WIRE.md 4.1 (алфавит Crockford и
/// косметические дефисы), 8.5 (поля блоба), 7.2 (первое доверие).
///
/// Энроллмент это единственный момент, когда доверие СОЗДАЁТСЯ, а не
/// проверяется. Всё остальное в CSM/1 проверяет то, что здесь закреплено.
library;

import 'dart:typed_data';

import 'package:caramba_vpn/csm.dart'
    show
        CsmBootstrapBlob,
        CsmDocType,
        CsmError,
        CsmErrorCode,
        CsmMirror,
        CsmParsed,
        csmLinkPin,
        csmParse,
        csmPid;

import 'package:caramba_client/data/models/csm_profile.dart';

/// Длина кода энроллмента без дефисов: 8 символов префикса пина плюс 12
/// символов секрета (02-SPEC.md 9.2).
const int kCsmEnrollCodeLength = 20;

/// Сколько символов `link_pin` свёрнуто в код. 8 символов Crockford это 40 бит.
const int kCsmEnrollCodePinPrefix = 8;

/// Окно правдоподобия часов при энроллменте: десять лет от `BUILD_EPOCH`
/// (02-SPEC.md 5.4).
const int kCsmClockPlausibilityWindowSec = 315360000;

/// Секунда Unix, на которой собрана эта сборка. Задаётся при сборке
/// `--dart-define=BUILD_EPOCH=<unix seconds>`; ноль означает «не задано», и
/// тогда проверка правдоподобия часов не может состояться.
const int kCsmBuildEpoch = int.fromEnvironment('BUILD_EPOCH');

/// Алфавит Crockford после свёртки `I`, `L` -> `1` и `O` -> `0`.
const String _crockfordAlphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

/// Нормализует введённый вручную код: убирает дефисы и пробелы, поднимает
/// регистр, сворачивает `I`/`L`/`O`. Возвращает `null`, если после этого
/// остался символ вне алфавита или длина не та (02-SPEC.md 9.8).
String? csmNormalizeEnrollCode(String raw) {
  final sb = StringBuffer();
  for (final unit in raw.toUpperCase().codeUnits) {
    final ch = String.fromCharCode(unit);
    if (ch == '-' || ch == ' ') {
      continue;
    }
    final folded = switch (ch) {
      'I' || 'L' => '1',
      'O' => '0',
      _ => ch,
    };
    if (!_crockfordAlphabet.contains(folded)) {
      return null;
    }
    sb.write(folded);
  }
  final out = sb.toString();
  return out.length == kCsmEnrollCodeLength ? out : null;
}

/// Префикс пина, свёрнутый в код. `null`, если код не нормализуется.
String? csmEnrollCodePinPrefix(String raw) {
  final code = csmNormalizeEnrollCode(raw);
  return code?.substring(0, kCsmEnrollCodePinPrefix);
}

/// Проверяет, что префикс кода согласуется с известным `link_pin`.
///
/// Клиент ОБЯЗАН делать эту проверку, когда пин у него есть из QR или блоба, и
/// ОБЯЗАН всегда проверять полный пин против первого ключевого документа
/// (02-SPEC.md 9.2). Несовпадение это жёсткая ошибка без пути «всё равно
/// продолжить».
bool csmEnrollCodeMatchesPin(String code, String linkPin) {
  final prefix = csmEnrollCodePinPrefix(code);
  if (prefix == null || linkPin.length < kCsmEnrollCodePinPrefix) {
    return false;
  }
  return prefix == linkPin.substring(0, kCsmEnrollCodePinPrefix).toUpperCase();
}

/// Правдоподобны ли часы устройства в момент энроллмента.
///
/// При энроллменте `clock_trusted` ложно, поэтому клауза перекоса V11 и весь
/// V12 инертны, и противник в пути мог бы подсунуть произвольно старый
/// ключевой документ. Окно `BUILD_EPOCH` это то, что ограничивает первое
/// доверие сроком жизни самого ключевого документа (02-SPEC.md 5.4).
///
/// Когда часы неправдоподобны, клиент ОБЯЗАН отказать в энроллменте, назвать
/// причиной неверные часы устройства, НЕ энроллиться вслепую и НЕ выставлять
/// часы сам.
bool csmClockPlausible(int nowSec, {int buildEpoch = kCsmBuildEpoch}) {
  if (buildEpoch <= 0) {
    return false;
  }
  return nowSec >= buildEpoch &&
      nowSec <= buildEpoch + kCsmClockPlausibilityWindowSec;
}

/// Ссылка энроллмента CSM/1: origin панели, код и, где он есть, `link_pin`.
///
/// `carambaconnect://enroll?panel=<https>&code=<invite>&k=<link_pin>`.
/// Параметр `k` добавлен аддитивно: старый клиент читает только `panel` и
/// `code` и не ломается (02-SPEC.md 9.8).
class CsmEnrollLink {
  const CsmEnrollLink({required this.origin, required this.code, this.linkPin});

  /// HTTPS-origin панели. `http://` не принимается: INV-8, единственное
  /// исключение это `.onion`, который самоаутентичен.
  final String origin;

  /// Код энроллмента, нормализованный: без дефисов, верхний регистр.
  final String code;

  /// 20 символов base32 Crockford, когда ссылка их несёт.
  final String? linkPin;

  /// Пин пришёл в самой ссылке, то есть с того же origin, что и документы.
  /// Слабее, чем продиктованный вне полосы, и экран личности оператора обязан
  /// это говорить (INV-18).
  CsmPinOrigin get pinOrigin => CsmPinOrigin.inApp;

  /// Согласуется ли префикс кода с пином из этой же ссылки.
  bool get codeMatchesPin {
    final pin = linkPin;
    return pin == null || csmEnrollCodeMatchesPin(code, pin);
  }

  static CsmEnrollLink? tryParse(String raw) {
    final uri = Uri.tryParse(raw.trim());
    if (uri == null || uri.scheme.toLowerCase() != 'carambaconnect') {
      return null;
    }
    final action = uri.host.isNotEmpty
        ? uri.host
        : (uri.pathSegments.isNotEmpty ? uri.pathSegments.first : '');
    if (action.toLowerCase() != 'enroll') {
      return null;
    }
    return fromParts(
      origin: uri.queryParameters['panel'] ?? '',
      code: uri.queryParameters['code'] ?? '',
      linkPin: uri.queryParameters['k'],
    );
  }

  static CsmEnrollLink? fromParts({
    required String origin,
    required String code,
    String? linkPin,
  }) {
    final normalizedOrigin = csmNormalizeOrigin(origin);
    if (normalizedOrigin == null) {
      return null;
    }
    final normalizedCode = csmNormalizeEnrollCode(code);
    if (normalizedCode == null) {
      return null;
    }
    final pin = linkPin == null ? null : csmNormalizeEnrollCode(linkPin);
    return CsmEnrollLink(
      origin: normalizedOrigin,
      code: normalizedCode,
      linkPin: pin,
    );
  }
}

/// Нормализует origin до `https://host[:port]`.
///
/// `http://` отвергается для любой выборки манифеста, конфигурации, правил или
/// geo. Единственное исключение без TLS это `.onion`, потому что луковые
/// адреса самоаутентичны (INV-8, 02-SPEC.md 8.10).
String? csmNormalizeOrigin(String raw) {
  var v = raw.trim();
  if (v.isEmpty) {
    return null;
  }
  if (!v.contains('://')) {
    v = 'https://$v';
  }
  final uri = Uri.tryParse(v);
  if (uri == null || uri.host.isEmpty) {
    return null;
  }
  final scheme = uri.scheme.toLowerCase();
  final host = uri.host.toLowerCase();
  final isOnion = host.endsWith('.onion');
  if (scheme != 'https' && !(scheme == 'http' && isOnion)) {
    return null;
  }
  final port = uri.hasPort ? ':${uri.port}' : '';
  return '$scheme://$host$port';
}

/// Почему энроллмент отказан. Каждая причина это отдельный экран и отдельный
/// текст; ни у одной нет пути «всё равно продолжить» (02-SPEC.md 9.9).
enum CsmEnrollFailure {
  /// Байты вообще не кадр CSM/1 или не бутстрап-блоб.
  notABlob,

  /// `sha256(rk)[0..12]` не равен продиктованному вне полосы пину. Жёсткая
  /// ошибка, профиль выбрасывается, повтор против того же origin не
  /// предлагается.
  pinMismatch,

  /// Код не сворачивает в себя префикс пина, который несёт этот же блоб.
  codePinMismatch,

  /// `org` блоба не https и не `.onion`.
  insecureOrigin,

  /// Часы устройства вне окна правдоподобия `BUILD_EPOCH`.
  implausibleClock,
}

/// Разобранный и проверенный против пина бутстрап-блоб.
class CsmBootstrap {
  const CsmBootstrap({
    required this.origin,
    required this.code,
    required this.rootPublicKey,
    required this.linkPin,
    required this.pid,
    required this.mirrors,
    this.operatorName,
  });

  final String origin;
  final String code;
  final Uint8List rootPublicKey;

  /// Пин, посчитанный ИЗ `rk` блоба, а не взятый из него: в блобе такого поля
  /// нет и быть не должно.
  final String linkPin;

  /// `sha256(root_pk)[0..8]`, hex.
  final String pid;

  final List<CsmMirror> mirrors;

  /// Инертное отображаемое имя оператора. Никогда не ключ, никогда не эхо.
  final String? operatorName;

  /// Блоб раздают вне полосы: печатают, шлют почтой, фотографируют. Поэтому
  /// пин, установленный из него, вне полосы и есть (INV-18).
  CsmPinOrigin get pinOrigin => CsmPinOrigin.outOfBand;

  CsmPin toPin({required int nowMs}) => CsmPin(
    pid: pid,
    linkPin: linkPin,
    origin: pinOrigin,
    establishedMs: nowMs,
  );
}

/// Результат разбора блоба: либо готовый [CsmBootstrap], либо причина отказа.
class CsmBootstrapResult {
  const CsmBootstrapResult.ok(this.bootstrap) : failure = null, code = null;

  const CsmBootstrapResult.failed(this.failure, {this.code}) : bootstrap = null;

  final CsmBootstrap? bootstrap;
  final CsmEnrollFailure? failure;

  /// Код реестра 03-WIRE.md 6.6, когда отказ пришёл из разбора кадра.
  final CsmErrorCode? code;

  bool get isOk => bootstrap != null;
}

/// Разбирает бутстрап-блоб `0x05` и устанавливает пин.
///
/// [expectedLinkPin] это пин, продиктованный вне полосы. Когда он задан,
/// `sha256(rk)[0..12]` ОБЯЗАН ему равняться, иначе жёсткий отказ без пути
/// «всё равно продолжить» (02-SPEC.md 9.7).
///
/// Подпись блоба здесь НЕ проверяется: это делает `CsmVerifier` с якорем
/// `link_pin` по правилу первого доверия 03-WIRE.md 7.2, и разделение
/// намеренное. Разбор решается целиком по байтам; проверка требует якоря.
CsmBootstrapResult csmParseBootstrap(
  Uint8List frame, {
  String? expectedLinkPin,
}) {
  final CsmParsed parsed;
  try {
    parsed = csmParse(frame);
  } on CsmError catch (e) {
    return CsmBootstrapResult.failed(CsmEnrollFailure.notABlob, code: e.code);
  }
  if (parsed.frame.docType != CsmDocType.bootstrapBlob) {
    return const CsmBootstrapResult.failed(CsmEnrollFailure.notABlob);
  }
  final doc = parsed.document;
  if (doc is! CsmBootstrapBlob) {
    return const CsmBootstrapResult.failed(CsmEnrollFailure.notABlob);
  }

  final linkPin = csmLinkPin(doc.rootKey);
  if (expectedLinkPin != null &&
      linkPin.toUpperCase() != expectedLinkPin.trim().toUpperCase()) {
    return const CsmBootstrapResult.failed(CsmEnrollFailure.pinMismatch);
  }
  if (!csmEnrollCodeMatchesPin(doc.code, linkPin)) {
    return const CsmBootstrapResult.failed(CsmEnrollFailure.codePinMismatch);
  }
  final origin = csmNormalizeOrigin(doc.origin);
  if (origin == null) {
    return const CsmBootstrapResult.failed(CsmEnrollFailure.insecureOrigin);
  }

  return CsmBootstrapResult.ok(
    CsmBootstrap(
      origin: origin,
      code: csmNormalizeEnrollCode(doc.code) ?? doc.code,
      rootPublicKey: doc.rootKey,
      linkPin: linkPin,
      pid: _hex(csmPid(doc.rootKey)),
      mirrors: doc.mirrors,
      operatorName: doc.operatorName,
    ),
  );
}

/// Заводит состояние CSM профиля из проверенного блоба. Профиль входит в
/// `pinning`: пин установлен, ключевой документ ещё не получен.
CsmProfileState csmProfileFromBootstrap(
  CsmBootstrap bootstrap, {
  required int nowMs,
}) => CsmProfileState(
  pin: bootstrap.toPin(nowMs: nowMs),
  stage: CsmProfileStage.pinning,
  operatorName: bootstrap.operatorName,
);

/// Заводит состояние CSM профиля из ссылки энроллмента, несущей пин.
///
/// Без `k=` пина нет, и профиль CSM завести нельзя: закреплять нечего.
/// Возвращает `null` именно в этом случае.
CsmProfileState? csmProfileFromLink(CsmEnrollLink link, {required int nowMs}) {
  final pin = link.linkPin;
  if (pin == null || !link.codeMatchesPin) {
    return null;
  }
  return CsmProfileState(
    pin: CsmPin(
      // pid выводится из корневого ключа, а ключа у ссылки нет: он приедет с
      // первым ключевым документом, и пока он не приехал, pid пуст.
      pid: '',
      linkPin: pin,
      origin: link.pinOrigin,
      establishedMs: nowMs,
    ),
    stage: CsmProfileStage.pinning,
  );
}

/// Липкое правило CSM, INV-13.
///
/// Профиль, закрепивший корневой ключ, НЕ откатывается к непроверяемому
/// legacy-поведению ни по какой причине: ни из-за отсутствующего `cap`, ни из-за
/// 404 на маршруте CSM, ни из-за того, что оператор откатил панель. Отсутствие
/// `cap` на таком профиле это жёсткая недиссмиссабельная ошибка.
///
/// Возвращает `true`, когда клиенту НЕЛЬЗЯ работать по legacy-пути.
bool csmStickyRuleBlocksLegacy(CsmProfileState? csm) {
  if (csm == null) {
    return false;
  }
  return csm.stage.isPinned;
}

/// Наступила ли жёсткая ошибка липкого правила: пин есть, а `cap` нет.
bool csmHardCapabilityError(CsmProfileState? csm) {
  if (csm == null) {
    return false;
  }
  return csm.stage.isPinned && csm.missingCapability;
}

/// Отмечает на профиле, что пришедший документ не нёс `cap`.
///
/// На незакреплённом профиле это просто факт; на закреплённом это состояние,
/// из которого пользователь не может выйти нажатием.
CsmProfileState csmMarkMissingCapability(CsmProfileState csm) =>
    csm.copyWith(missingCapability: true);

String _hex(List<int> bytes) {
  final sb = StringBuffer();
  for (final b in bytes) {
    sb.write(b.toRadixString(16).padLeft(2, '0'));
  }
  return sb.toString();
}

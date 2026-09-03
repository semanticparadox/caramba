/// Мост регистрации и обновления CSM/1: из ядра в состояние профиля.
///
/// Нормативно: 02-SPEC.md 9 (регистрация целиком), 7.2 (первое доверие), 5.4
/// (временной пол), INV-16 (отказ выборки не отменяет конфигурацию), INV-19
/// (состояние проверки документов видно пользователю).
///
/// Регистрация и цикл выборки живут В ЯДРЕ, а не здесь: управляющий слой на
/// Dart не имеет права открывать собственные сокеты к оператору, иначе
/// регистрация, вход, обновление токена и настройки обходят лестницу
/// транспортов, и приложение вырождается в ступень R0, пока ядро бодро лезет по
/// лестнице за конфигурацией, которую ему больше нечем изменить
/// (02-SPEC.md 8.9). Этот файл только переносит ПРОВЕРЕННЫЙ ядром результат в
/// состояние профиля.
///
/// Без него `CsmNotifier.anchor` не звал никто, профиль навсегда оставался в
/// стадии `pinned`, действующий набор возможностей падал до пустого, запись
/// настроек не уходила, отпечаток каталога был пуст, а история попыток
/// оставалась пустым списком: весь слой CSM был инертен в собранном
/// приложении.
library;

import 'dart:convert';

import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';

/// Чем закончилась попытка закрепиться на проверенном ключевом документе.
enum CsmAnchorOutcome {
  /// Профиль закреплён: `pid` посчитан, ключевой документ проверен, временной
  /// пол установлен.
  anchored,

  /// Ядро CSM недоступно в этой сборке или мост не отвечает. Это НЕ ошибка
  /// приложения: сборка без ABI v3 честно говорит, что регистрации у неё нет.
  unavailable,

  /// Ядро ответило, но регистрация не состоялась (код отвергнут, пин не сошёлся,
  /// сеть не дала пройти ни по одной ступени).
  refused,

  /// Ответ ядра не разобрался. Отдельный исход, потому что «регистрация не
  /// прошла» и «ответ испорчен» это разные утверждения.
  malformed,
}

/// Результат одной попытки с сырым снимком, когда он был.
class CsmAnchorResult {
  const CsmAnchorResult(this.outcome, {this.snapshot = '', this.error = ''});

  final CsmAnchorOutcome outcome;

  /// Снимок состояния, как его отдало ядро. Пусто, когда его не было.
  final String snapshot;

  /// Причина отказа. Инертный текст, наружу пользователю его показывает
  /// вызывающий, а не этот файл.
  final String error;

  bool get ok => outcome == CsmAnchorOutcome.anchored;
}

/// Регистрирует профиль у оператора и закрепляет проверенный результат.
///
/// Порядок здесь и есть смысл функции: сначала ядро проходит регистрацию по
/// лестнице и ПРОВЕРЯЕТ ключевой документ против закреплённого пина, и только
/// потом состояние профиля переходит в `anchored`. Обратный порядок означал бы
/// профиль, объявленный закреплённым до того, как хоть что-то проверено.
Future<CsmAnchorResult> csmEnrollAndAnchor({
  required VpnConnection connection,
  required CsmNotifier notifier,
  String origin = '',
  String code = '',
  String linkPin = '',
  String blobB64 = '',
  String subscriptionDomain = '',
  String accountJwt = '',
  int? nowMs,
}) async {
  final String raw;
  try {
    raw = await connection.csmEnroll(
      origin: origin,
      code: code,
      linkPin: linkPin,
      blobB64: blobB64,
      subscriptionDomain: subscriptionDomain,
      accountJwt: accountJwt,
    );
  } on Object catch (e) {
    return CsmAnchorResult(CsmAnchorOutcome.refused, error: '$e');
  }
  return csmAnchorFromSnapshot(notifier: notifier, raw: raw, nowMs: nowMs);
}

/// Один цикл выборки документов.
///
/// Отказ НЕ означает потерю конфигурации: профиль остаётся на кешированных
/// документах и продолжает подключать (INV-16). Поэтому неудачный цикл здесь
/// возвращает исход, а не откатывает состояние.
Future<CsmAnchorResult> csmRefreshAndAnchor({
  required VpnConnection connection,
  required CsmNotifier notifier,
  int timeoutSec = 30,
  int? nowMs,
}) async {
  final String raw;
  try {
    raw = await connection.csmRefresh(timeoutSec: timeoutSec);
  } on Object catch (e) {
    return CsmAnchorResult(CsmAnchorOutcome.refused, error: '$e');
  }
  return csmAnchorFromSnapshot(notifier: notifier, raw: raw, nowMs: nowMs);
}

/// Переносит проверенный снимок ядра в состояние профиля.
///
/// Закрепление происходит ТОЛЬКО когда снимок несёт и `pid`, и присутствующий
/// ключевой документ: закрепить профиль на `pid` без документа, которым он
/// посчитан, значит объявить проверенным то, что не проверялось.
Future<CsmAnchorResult> csmAnchorFromSnapshot({
  required CsmNotifier notifier,
  required String raw,
  int? nowMs,
}) async {
  if (raw.trim().isEmpty) {
    return const CsmAnchorResult(CsmAnchorOutcome.unavailable);
  }
  final Object? decoded;
  try {
    decoded = jsonDecode(raw);
  } on FormatException {
    return CsmAnchorResult(CsmAnchorOutcome.malformed, snapshot: raw);
  }
  if (decoded is! Map) {
    return CsmAnchorResult(CsmAnchorOutcome.malformed, snapshot: raw);
  }
  final err = decoded['error'];
  if (err is String && err.isNotEmpty) {
    return CsmAnchorResult(CsmAnchorOutcome.refused, snapshot: raw, error: err);
  }
  final pid = '${decoded['pid'] ?? ''}';
  final key = csmKeyDocumentFromCoreSnapshot(decoded, nowMs: nowMs);
  if (pid.isEmpty || key == null) {
    return CsmAnchorResult(
      CsmAnchorOutcome.refused,
      snapshot: raw,
      error: 'ядро не отдало проверенный ключевой документ',
    );
  }
  await notifier.anchor(
    pid: pid,
    keyDocument: key,
    timeFloorSec: (decoded['time_floor'] as num?)?.toInt() ?? 0,
  );
  return CsmAnchorResult(CsmAnchorOutcome.anchored, snapshot: raw);
}

/// Ключевой документ из снимка ядра, или `null`, когда его там нет.
///
/// `present` ложно означает, что ядро ключевого документа не держит, и это НЕ
/// повод собрать пустую запись: документ, которого не было, не имеет права
/// выглядеть на экране INV-19 проверенным.
CsmDocumentRecord? csmKeyDocumentFromCoreSnapshot(
  Map<Object?, Object?> snap, {
  int? nowMs,
}) {
  final key = snap['key'];
  if (key is! Map || key['present'] != true) {
    return null;
  }
  final signers = <String>[];
  final rawSigners = key['signers'];
  if (rawSigners is List) {
    for (final s in rawSigners) {
      if (s is String && s.isNotEmpty) {
        signers.add(s);
      }
    }
  }
  return CsmDocumentRecord(
    // 0x01, ключевой документ (03-WIRE.md 6.1).
    docType: 0x01,
    version: (key['ver'] as num?)?.toInt() ?? 0,
    issuedSec: (key['iat'] as num?)?.toInt() ?? 0,
    expiresSec: (key['exp'] as num?)?.toInt() ?? 0,
    signerFingerprints: List<String>.unmodifiable(signers),
    verifiedAtMs: nowMs ?? DateTime.now().millisecondsSinceEpoch,
    // Ключевой документ не несёт области отметки максимума версий
    // (02-SPEC.md 5.1), и выдумывать её здесь нечего.
    verdict: 'ok',
  );
}

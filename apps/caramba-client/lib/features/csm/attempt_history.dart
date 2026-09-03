/// Живая история попыток по ступеням лестницы (INV-17, 02-SPEC.md 8.8).
///
/// Хранит последние [kCsmAttemptHistoryLimit] записей на профиль. Каждая несёт
/// ступень, хост либо непрозрачную метку зеркала, время старта, исход и код
/// отказа из реестра 03-WIRE.md 6.6, когда он был.
///
/// Она ЛОКАЛЬНА и НИКОГДА не выгружается. 02-SPEC.md 7.10 пункт 2 запрещает
/// сообщать оператору, какая ступень принесла запрос: это живая карта того,
/// какие обходные пути ещё работают, по устройству и по автономной системе,
/// отданная стороне, которую могут принудить. Поэтому история живёт в памяти
/// приложения, рядом с экраном, который её показывает, и не имеет ни одного
/// пути наружу.
library;

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/csm_profile.dart';

/// Сколько записей держим. 02-SPEC.md 8.8: последние 200 на профиль.
const int kCsmAttemptHistoryLimit = 200;

/// Исход одной попытки.
enum CsmAttemptOutcome {
  /// Ступень принесла кадр, и он проверился.
  ok,

  /// Ступень не принесла ничего либо принесла то, что не прошло разбор или
  /// проверку. Код отказа лежит в [CsmAttempt.errorCode].
  failed,

  /// Ступень пропущена: выключена пользователем либо недоступна.
  skipped,
}

/// Одна попытка на одной ступени.
class CsmAttempt {
  const CsmAttempt({
    required this.rung,
    required this.startedMs,
    required this.outcome,
    this.host = '',
    this.errorCode,
    this.durationMs,
    this.bytes = 0,
    this.status = 0,
  });

  final CsmRung rung;

  /// Хост либо непрозрачная метка зеркала. Пустая строка у R0 и R6: диска и
  /// человека не бывает по имени хоста.
  final String host;

  final int startedMs;
  final CsmAttemptOutcome outcome;

  /// Код из реестра 03-WIRE.md 6.6. `null`, когда отказа не было.
  final String? errorCode;

  final int? durationMs;

  /// Сколько байт попытка принесла, включая тело, которое потом не прошло
  /// проверку. Ноль означает «не принесла ничего», и это разные вещи: пустой
  /// ответ и подделанный кадр обязаны выглядеть на экране по-разному
  /// (02-SPEC.md 8.8).
  final int bytes;

  /// Код состояния HTTP, когда ответ пришёл. Ноль означает, что до ответа не
  /// дошло.
  final int status;
}

/// Кольцевой буфер попыток. Пишет сюда ходок по лестнице; экран только читает.
class CsmAttemptHistory extends StateNotifier<List<CsmAttempt>> {
  CsmAttemptHistory() : super(const <CsmAttempt>[]);

  /// Записывает попытку. Самая свежая идёт первой: экран читается сверху вниз.
  void record(CsmAttempt attempt) {
    final next = <CsmAttempt>[attempt, ...state];
    if (next.length > kCsmAttemptHistoryLimit) {
      next.removeRange(kCsmAttemptHistoryLimit, next.length);
    }
    state = List<CsmAttempt>.unmodifiable(next);
  }

  /// Смена профиля обнуляет историю: она хранится на профиль.
  void clear() => state = const <CsmAttempt>[];
}

/// История попыток активного профиля.
final csmAttemptHistoryProvider =
    StateNotifierProvider<CsmAttemptHistory, List<CsmAttempt>>(
      (ref) => CsmAttemptHistory(),
    );

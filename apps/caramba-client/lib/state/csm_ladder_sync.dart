/// Мост истории попыток и состояния ступеней из ядра в состояние приложения.
///
/// Нормативно: 02-SPEC.md 8.1 и 8.3 (лестница), 8.8 (что пользователь обязан
/// видеть), INV-17 (живая история попыток на экране транспортов), 7.10 пункт 2
/// (история ЛОКАЛЬНА и оператору не сообщается никогда).
///
/// Лестницей ходит ядро, поэтому попытки записывает тоже оно: у слоя Dart нет
/// ни сокета к оператору, ни знания о том, какое зеркало было выбрано. Без
/// этого моста `CsmAttemptHistory` никто не наполняет, экран показывает пустой
/// список, и INV-17 оказывается декорацией: приложение обещает показать каждую
/// попытку и не показывает ни одной.
///
/// Направление одностороннее: сюда история поднимается, отсюда она не уходит
/// НИКУДА. Живая карта того, какие обходные пути ещё работают, по устройству и
/// по автономной системе, отданная стороне, которую могут принудить, это ровно
/// то, что 02-SPEC.md 7.10 запрещает.
library;

import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/features/csm/attempt_history.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Чем закончился один подъём истории.
enum CsmLadderSyncOutcome {
  /// Ядро отдало разобранный ответ.
  ok,

  /// Ядро CSM в этой сборке недоступно либо ответило пустой строкой. Это НЕ
  /// ошибка: сборка без ABI v3 честно говорит, что лестницы у неё нет.
  unavailable,

  /// Ядро ответило, но ответ не разобрался. Отдельный исход, потому что
  /// «нечего показывать» и «ответ испорчен» это разные утверждения.
  malformed,
}

/// Поднимает состояние лестницы из ядра.
///
/// Одноразовый вызов [pump]: экран транспортов зовёт его при открытии и по
/// таймеру, пока он виден. Опроса в фоне здесь нет намеренно, история нужна
/// ровно тому экрану, который её показывает.
class CsmLadderSync {
  CsmLadderSync(this._ref);

  final Ref _ref;

  /// Опознавательная строка самой свежей уже записанной попытки. Ядро держит
  /// кольцевой буфер и отдаёт его целиком, поэтому дозаписывать надо только
  /// хвост; иначе каждый подъём удваивал бы историю на экране.
  String? _lastKey;

  Future<CsmLadderSyncOutcome> pump() async {
    final String raw;
    try {
      raw = await _ref.read(vpnConnectionProvider).csmLadder();
    } on Object {
      // Ядро без ABI v3 либо мост, которого на этой платформе нет. Экран
      // остаётся с тем, что уже поднято, и ничего не выдумывает.
      return CsmLadderSyncOutcome.unavailable;
    }
    if (raw.trim().isEmpty) {
      return CsmLadderSyncOutcome.unavailable;
    }
    final Object? decoded;
    try {
      decoded = jsonDecode(raw);
    } on FormatException {
      return CsmLadderSyncOutcome.malformed;
    }
    if (decoded is! Map) {
      return CsmLadderSyncOutcome.malformed;
    }
    if (decoded['error'] is String) {
      return CsmLadderSyncOutcome.unavailable;
    }

    final rungs = decoded['rungs'];
    if (rungs is List) {
      _ref.read(csmTransportFactsProvider.notifier).state =
          csmTransportFactsFromRungs(rungs);
    }

    // Резервный путь R5 через локальный Tor и ступень, принесшая последнюю
    // принятую конфигурацию. Оба поля ядро отдаёт всегда; их отсутствие в
    // ответе это сборка ядра старше этого экрана, и тогда состояние остаётся
    // «не искали», а не выдумывается.
    _ref.read(csmTorStatusProvider.notifier).state = csmTorStatusFromCoreJson(
      decoded['tor'],
    );
    _ref.read(csmDeliveredRungProvider.notifier).state = csmRungFromCoreName(
      decoded['delivered'],
    );

    final history = decoded['history'];
    if (history is! List) {
      return CsmLadderSyncOutcome.malformed;
    }
    // Ядро отдаёт историю в хронологическом порядке, самая старая первой.
    final parsed = <CsmAttempt>[];
    final keys = <String>[];
    for (final entry in history) {
      final attempt = csmAttemptFromCoreJson(entry);
      if (attempt == null) {
        continue;
      }
      parsed.add(attempt);
      keys.add(csmAttemptKey(entry as Map<Object?, Object?>));
    }
    if (parsed.isEmpty) {
      return CsmLadderSyncOutcome.ok;
    }

    // Дозаписывается ХВОСТ после последней уже известной попытки. Когда её в
    // ответе нет, кольцевой буфер ядра успел провернуться или приложение
    // перезапустилось, и тогда берётся всё: показать меньше, чем ядро помнит,
    // значит скрыть попытку, которая была.
    var from = 0;
    final last = _lastKey;
    if (last != null) {
      final idx = keys.lastIndexOf(last);
      if (idx >= 0) {
        from = idx + 1;
      }
    }
    final notifier = _ref.read(csmAttemptHistoryProvider.notifier);
    for (var i = from; i < parsed.length; i++) {
      notifier.record(parsed[i]);
    }
    _lastKey = keys.last;
    return CsmLadderSyncOutcome.ok;
  }

  /// Сбрасывает курсор. Зовётся при смене профиля вместе с
  /// [CsmAttemptHistory.clear]: история хранится на профиль.
  void reset() => _lastKey = null;
}

final csmLadderSyncProvider = Provider<CsmLadderSync>(CsmLadderSync.new);

/// Опознавательная строка одной попытки. Ядро идентификатора не выдаёт, а
/// момент старта в миллисекундах вместе со ступенью, хостом и исходом
/// различает записи достаточно, чтобы хвост находился на своём месте.
String csmAttemptKey(Map<Object?, Object?> raw) {
  // Поля code, status и bytes помечены в ядре omitempty, поэтому нулевое
  // значение приходит как ОТСУТСТВИЕ ключа. Нормализация к пустой строке
  // держит форму ключа одинаковой в обоих случаях; без неё та же попытка,
  // у которой поле впервые стало ненулевым, получила бы ключ другой формы и
  // дозапись хвоста поехала бы.
  String f(String name) => raw[name] == null ? '' : '${raw[name]}';
  return '${f('rung')}|${f('start')}|${f('host')}|${f('outcome')}'
      '|${f('code')}|${f('status')}|${f('bytes')}|${f('millis')}';
}

/// Разбирает одну запись истории ядра.
///
/// Возвращает `null`, когда запись не несёт ступени или момента старта: строка
/// без них не является попыткой, и рисовать её пустой значило бы показать
/// событие, которого не было.
CsmAttempt? csmAttemptFromCoreJson(Object? raw) {
  if (raw is! Map) {
    return null;
  }
  final rung = CsmRung.fromId((raw['rung'] as num?)?.toInt() ?? -1);
  if (rung == null) {
    return null;
  }
  final started = DateTime.tryParse('${raw['start'] ?? ''}');
  if (started == null) {
    return null;
  }
  final outcome = '${raw['outcome'] ?? ''}';
  final code = '${raw['code'] ?? ''}';
  return CsmAttempt(
    rung: rung,
    host: '${raw['host'] ?? ''}',
    startedMs: started.millisecondsSinceEpoch,
    // Исход `budget` это ступень, которую бюджет соединения не дал даже
    // попробовать: запроса не было. Рисовать её как отказ значит показать
    // отказ, которого не происходило, а экран умеет говорить "пропущена".
    outcome: switch (outcome) {
      'ok' => CsmAttemptOutcome.ok,
      'budget' => CsmAttemptOutcome.skipped,
      _ => CsmAttemptOutcome.failed,
    },
    // Код из реестра 03-WIRE.md 6.6, когда он был; иначе исход как он назван
    // ядром. Пустая причина у неудачной попытки означала бы отказ, о котором
    // приложение промолчало, а INV-17 требует обратного.
    errorCode: outcome == 'ok' ? null : (code.isNotEmpty ? code : outcome),
    durationMs: (raw['millis'] as num?)?.toInt(),
    bytes: (raw['bytes'] as num?)?.toInt() ?? 0,
    status: (raw['status'] as num?)?.toInt() ?? 0,
  );
}

/// Выводит факты о транспорте из причин, которые назвало ядро.
///
/// Это те два значения, которые слой Dart знать не может: поддерживает ли эта
/// сборка выборку через поднятый туннель (R4) и введён ли пользовательский
/// прокси (R5). Пока они брались из умолчаний параметров, экран рисовал R4 как
/// `platform_unsupported`, а R5 как `not_configured` независимо от того, что
/// настроено на самом деле.
CsmTransportFacts csmTransportFactsFromRungs(List<Object?> rungs) {
  var tunnel = false;
  var proxy = false;
  for (final entry in rungs) {
    if (entry is! Map) {
      continue;
    }
    final rung = (entry['rung'] as num?)?.toInt();
    if (rung == CsmRung.tunnel.id) {
      tunnel = _rungHasPath(entry);
    } else if (rung == CsmRung.userProxy.id) {
      proxy = _rungHasPath(entry);
    }
  }
  return CsmTransportFacts(
    tunnelFetchSupported: tunnel,
    proxyConfigured: proxy,
  );
}

/// Есть ли у ступени путь, по словам самого ядра.
///
/// Вывод делается по `enabled` плюс ЗАКРЫТЫЙ список причин, а не отрицанием
/// одной причины. Отрицание было перевёрнутой логикой в самом обычном случае:
/// ядро под тегом mihomo никогда не отвечает `platform_unsupported` за R4, а
/// при опущенном туннеле ставит `not_configured`, и проверка
/// `reason != 'platform_unsupported'` объявляла ступень доступной ровно тогда,
/// когда ядро в ней отказало. То же с R5, где `user_disabled` превращался в
/// «прокси введён».
///
/// Единственная причина, которая НЕ означает отсутствие пути, это
/// `user_disabled`: путь есть, его выключил пользователь, и экрану надо сказать
/// именно это, а не «сборка так не умеет».
bool _rungHasPath(Map<Object?, Object?> entry) {
  if (entry['enabled'] == true) {
    return true;
  }
  return '${entry['reason'] ?? ''}' == 'user_disabled';
}

/// Что ядро знает про локальный Tor на этом устройстве.
///
/// Словарь закрытый и повторяет `transport.TorState`. «Не искали» это отдельное
/// состояние, а не вежливая форма «не нашли»: экран обязан уметь произнести обе
/// строки, иначе выключенный резерв и отсутствующий Orbot выглядят одинаково.
enum CsmTorState {
  /// Ступень R5 выключена, поэтому локальный порт не проверяли.
  unknown,

  /// Локальный SOCKS5 отвечает, адрес подставлен в R5.
  ready,

  /// Искали и не нашли. Причина лежит в [CsmTorStatus.detail].
  absent,

  /// Ступень занята прокси, который ввёл сам пользователь.
  superseded,

  /// На этой платформе стороннее приложение не отдаёт SOCKS на петлю.
  unsupported,
}

/// Состояние резервного пути R5 через локальный Tor.
///
/// ВАЖНО про то, чего этот путь не даёт. Через Tor берётся только подписка и
/// конфигурация; трафик туннеля через него не идёт, и сеанс VPN от этого
/// анонимным не становится. Экран обязан говорить ровно это, а не «анонимно».
class CsmTorStatus {
  const CsmTorStatus({
    this.state = CsmTorState.unknown,
    this.addr = '',
    this.detail = '',
    this.checkedAtSec = 0,
  });

  static const CsmTorStatus none = CsmTorStatus();

  final CsmTorState state;

  /// Адрес, который реально стоит в ступени. Непуст только при [ready].
  final String addr;

  /// Причина словами ядра. Строк оператора здесь нет (INV-10).
  final String detail;

  /// Момент последней пробы, unix-секунды. Ноль означает «пробы не было», и
  /// это не то же самое, что «не нашли».
  final int checkedAtSec;
}

/// Разбирает блок `tor` ответа ядра. Отсутствие блока даёт [CsmTorStatus.none]:
/// сборка ядра, которая про резервный путь не знает, не имеет права выглядеть
/// как устройство без Orbot.
CsmTorStatus csmTorStatusFromCoreJson(Object? raw) {
  if (raw is! Map) {
    return CsmTorStatus.none;
  }
  return CsmTorStatus(
    state: switch ('${raw['state'] ?? ''}') {
      'ready' => CsmTorState.ready,
      'absent' => CsmTorState.absent,
      'superseded' => CsmTorState.superseded,
      'unsupported' => CsmTorState.unsupported,
      _ => CsmTorState.unknown,
    },
    addr: '${raw['addr'] ?? ''}',
    detail: '${raw['detail'] ?? ''}',
    checkedAtSec: (raw['checked_at'] as num?)?.toInt() ?? 0,
  );
}

/// Ступень по машинному имени из ядра (`RungID.Name`).
///
/// Возвращает `null` для пустой строки и для имён, которых этот экран не знает
/// (`onion` собран, но не реализован). Нарисовать вместо неизвестного имени
/// первую попавшуюся ступень значило бы соврать про то, что принесло
/// конфигурацию.
CsmRung? csmRungFromCoreName(Object? raw) => switch ('${raw ?? ''}') {
  'cached' => CsmRung.cached,
  'direct' => CsmRung.direct,
  'mirrors' => CsmRung.mirrors,
  'doh' => CsmRung.doh,
  'tunnel' => CsmRung.tunnel,
  'proxy' => CsmRung.userProxy,
  'out_of_band' => CsmRung.outOfBand,
  _ => null,
};

/// Состояние резервного пути через Tor, поднятое из ядра.
final csmTorStatusProvider = StateProvider<CsmTorStatus>(
  (ref) => CsmTorStatus.none,
);

/// Ступень, принесшая последнюю ПРИНЯТУЮ конфигурацию. `null` означает, что не
/// приносила ещё ни одна: это честное «неизвестно», а не R0.
final csmDeliveredRungProvider = StateProvider<CsmRung?>((ref) => null);

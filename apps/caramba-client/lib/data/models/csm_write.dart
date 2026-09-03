/// Запись настроек CSM/1: тело `want`, очередь и предобраз доказательства.
///
/// Нормативно: 02-SPEC.md 7.5 (трёхсторонняя семантика), 7.8 (локально сразу,
/// потом в очередь), 7.10 (что не пересекает границу никогда), 03-WIRE.md 13.6
/// (форма запроса, заголовки, предобраз `X-CSM-Proof`).
///
/// INV-15 живёт ЗДЕСЬ, а не на вызывающей стороне: `split.apps` не имеет ключа
/// в [CsmSettingKey], поэтому сериализатор физически не способен его написать.
/// Список установленных приложений это самая опознающая вещь, которую клиент
/// VPN мог бы выгрузить, и запрет не должен зависеть от дисциплины вызова.
library;

import 'dart:convert';
import 'dart:typed_data';

import 'package:caramba_vpn/csm.dart' show sha256;

import 'package:caramba_client/data/models/csm_settings.dart';

/// Канонический литерал пути для записи настроек. Никогда не полученный путь:
/// `api_routes` смонтирован дважды, и проверяющий, подписавший полученный путь,
/// отверг бы клиента, назвавшего другой монтаж (03-WIRE.md 13.6).
const String kCsmWritePathPreferences = '/api/v2/app/preferences';
const String kCsmWritePathEnrollCode = '/api/v2/app/csm/enroll/code';
const String kCsmWritePathEnrollDevice = '/api/v2/app/csm/enroll/device';

/// Что делает запись с одним ключом. Трёхсторонняя семантика 02-SPEC.md 7.5:
/// ключа нет в `want` -> не менять; значение это текст `default` -> сбросить к
/// значению оператора; любое другое значение -> поставить.
sealed class CsmWantOp {
  const CsmWantOp();
}

/// Поставить значение.
class CsmWantSet extends CsmWantOp {
  const CsmWantSet(this.value);

  final CsmSettingValue value;
}

/// Сбросить к значению оператора. Сентинел это текстовая строка `default` для
/// ЛЮБОГО ключа, какого бы типа он ни был: CBOR null запрещён строгим
/// профилем разбора (03-WIRE.md 3.1 C7), формы `["default"]` не существует.
class CsmWantReset extends CsmWantOp {
  const CsmWantReset();
}

/// Тело записи настроек.
///
/// Ключевое пространство то же, что у карты `pol` директивы, но БЕЗ обёртки
/// происхождения: происхождение решает панель, клиент его не заявляет.
class CsmSettingsWrite {
  const CsmSettingsWrite({
    required this.nonce,
    required this.deviceThumbprint,
    required this.want,
    this.ifMatchVersion,
  });

  /// 16 байт из криптографически стойкого источника, свежие на запрос.
  final Uint8List nonce;

  /// `dtp` этого устройства, 16 байт.
  final Uint8List deviceThumbprint;

  final Map<CsmSettingKey, CsmWantOp> want;

  /// `ver` директивы, которую запись правит. Уходит в заголовок `If-Match`
  /// десятичной строкой.
  final int? ifMatchVersion;

  /// Карта `want` для CBOR: ключ настройки -> значение или сентинел сброса.
  ///
  /// Здесь и проходит граница INV-15. `split.apps` не может тут появиться,
  /// потому что у него нет ключа в закрытом реестре, и назначить его нельзя.
  /// Вызывающая сторона не имеет возможности «просто добавить поле».
  Map<int, Object?> toWantMap() {
    final out = <int, Object?>{};
    for (final e in want.entries) {
      final key = e.key;
      final op = e.value;
      switch (op) {
        case CsmWantReset():
          out[key.wire] = kCsmDefaultSentinel;
        case CsmWantSet(value: final v):
          // Значение вне закрытого словаря не уходит на провод: INV-11
          // запрещает эхо того, что нельзя проверить.
          if (!csmValueInVocabulary(key, v)) {
            continue;
          }
          out[key.wire] = v.toJson();
      }
    }
    return out;
  }

  /// Заголовок `If-Match` или `null`, когда правится ничто.
  String? get ifMatchHeader => ifMatchVersion?.toString();
}

/// Предобраз доказательства `X-CSM-Proof` (03-WIRE.md 13.6).
///
/// ```
/// sha256("csm1-write" || 0x00 || method || 0x00 || path || 0x00 ||
///        sha256(request body))
/// ```
///
/// Подписывается ОДНА конструкция во всём CSM/1: и запись настроек, и оба тела
/// энроллмента, и доказательство перевыпуска ключа соглашения. Подписывающий
/// НЕ хеширует заранее, а отдаёт сообщение платформенному API, которое хеширует
/// само.
Uint8List csmWriteProofMessage({
  required String method,
  required String canonicalPath,
  required List<int> body,
}) {
  final out = <int>[
    ...utf8.encode('csm1-write'),
    0x00,
    // Метод входит в прообраз ДОСЛОВНО. Нормативная реализация на Go пишет
    // сюда []byte(method) как есть, и приведение регистра здесь означало бы
    // две реализации, собирающие разные байты из одного входа: у "put" вышел
    // бы прообраз, который проверяющий не воспроизведёт. Регистр это забота
    // вызывающего, и 03-WIRE.md 13.6 знает только PUT и POST.
    ...utf8.encode(method),
    0x00,
    ...utf8.encode(canonicalPath),
    0x00,
    ...sha256(body),
  ];
  return Uint8List.fromList(out);
}

/// Собирает карту `want` из очереди записей.
///
/// Ключ это номер поля директивы, значение типизировано так же, как карта `pol`
/// (03-WIRE.md 8.3): текст, беззнаковое целое, булево или массив текстов.
/// Сброс это текст `default` для ЛЮБОГО ключа, какого бы типа он ни был.
///
/// INV-15 держится и здесь по той же причине, что и в [CsmSettingsWrite]:
/// `split.apps` не имеет ключа в [CsmSettingKey], поэтому его нечем записать.
Map<int, Object?> csmWantMapFromQueue(Iterable<CsmQueuedWrite> queue) {
  final out = <int, Object?>{};
  for (final entry in queue) {
    switch (entry.op) {
      case CsmWantReset():
        out[entry.key.wire] = kCsmDefaultSentinel;
      case CsmWantSet(value: final v):
        // Значение вне закрытого словаря на провод не уходит: INV-11 запрещает
        // эхо того, что нельзя проверить.
        if (!csmValueInVocabulary(entry.key, v)) {
          continue;
        }
        out[entry.key.wire] = v.toJson();
    }
  }
  return out;
}

/// Одна запись в персистентной очереди (02-SPEC.md 7.8).
class CsmQueuedWrite {
  const CsmQueuedWrite({
    required this.key,
    required this.op,
    required this.queuedMs,
    this.nonceIssuedMs = 0,
  });

  final CsmSettingKey key;
  final CsmWantOp op;
  final int queuedMs;

  /// Когда для этой записи выдавался nonce. Запись, чей nonce старше 300
  /// секунд, ОБЯЗАНА быть переподписана свежим, а не отправлена протухшей.
  final int nonceIssuedMs;

  bool isStaleNonce(int nowMs) =>
      nonceIssuedMs == 0 || nowMs - nonceIssuedMs > 300 * 1000;

  /// Запись, не доставленная 7 суток, выбрасывается, и пользователю один раз
  /// говорят, что изменение осталось локальным.
  bool isExpired(int nowMs) => nowMs - queuedMs > 604800 * 1000;

  Map<String, Object?> toJson() => <String, Object?>{
    'key': key.wire,
    'reset': op is CsmWantReset,
    'value': op is CsmWantSet ? (op as CsmWantSet).value.toJson() : null,
    'queued_ms': queuedMs,
    'nonce_ms': nonceIssuedMs,
  };

  static CsmQueuedWrite? fromJson(Object? raw) {
    if (raw is! Map) {
      return null;
    }
    final key = CsmSettingKey.fromWire((raw['key'] as num?)?.toInt() ?? -1);
    if (key == null) {
      return null;
    }
    final CsmWantOp op;
    if (raw['reset'] == true) {
      op = const CsmWantReset();
    } else {
      final value = CsmSettingValue.fromJson(key.type, raw['value']);
      if (value == null || !csmValueInVocabulary(key, value)) {
        return null;
      }
      op = CsmWantSet(value);
    }
    return CsmQueuedWrite(
      key: key,
      op: op,
      queuedMs: (raw['queued_ms'] as num?)?.toInt() ?? 0,
      nonceIssuedMs: (raw['nonce_ms'] as num?)?.toInt() ?? 0,
    );
  }
}

/// Очередь записей: не глубже [kCsmWriteQueueDepth], со схлопыванием по ключу.
///
/// Вторая правка того же ключа ЗАМЕНЯЕТ первую, а не дописывается: панель
/// интересует последнее желаемое состояние, а не история нажатий.
class CsmWriteQueue {
  const CsmWriteQueue([this.entries = const <CsmQueuedWrite>[]]);

  static const CsmWriteQueue empty = CsmWriteQueue();

  final List<CsmQueuedWrite> entries;

  bool get isEmpty => entries.isEmpty;

  int get length => entries.length;

  CsmWriteQueue enqueue(CsmQueuedWrite write) {
    final out = entries.where((e) => e.key != write.key).toList();
    out.add(write);
    // Переполнение сбрасывает САМЫЕ СТАРЫЕ: свежее желание пользователя важнее
    // давнего, которое так и не ушло.
    while (out.length > kCsmWriteQueueDepth) {
      out.removeAt(0);
    }
    return CsmWriteQueue(List<CsmQueuedWrite>.unmodifiable(out));
  }

  /// Убирает доставленные и протухшие записи.
  CsmWriteQueue prune(int nowMs) => CsmWriteQueue(
    List<CsmQueuedWrite>.unmodifiable(
      entries.where((e) => !e.isExpired(nowMs)),
    ),
  );

  List<Object?> toJson() =>
      entries.map((e) => e.toJson()).toList(growable: false);

  static CsmWriteQueue fromJson(Object? raw) {
    if (raw is! List) {
      return CsmWriteQueue.empty;
    }
    return CsmWriteQueue(
      List<CsmQueuedWrite>.unmodifiable(
        raw.map(CsmQueuedWrite.fromJson).whereType<CsmQueuedWrite>(),
      ),
    );
  }
}

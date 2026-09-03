/// Отметки максимума версий профиля, 02-SPEC.md 5.1 и 8.8.3.
///
/// > Отметка ОБЯЗАНА жить ровно в одном хранилище, в процессе приложения.
///
/// Здесь это состояние CSM на профиле подключения, которое лежит одной записью
/// в платформенном secure storage (`ConnectionProfilesStore`). Второго дома у
/// отметки нет: два рабочих каталога это две отметки, а это дыра для отката, а
/// не защита в глубину.
///
/// Кадры для ступени R0 живут не здесь. По 8.8.3 они лежат в каталоге
/// поддержки приложения, `csm/<pid>/`, дословно как получены, и подделанный
/// кадр ловится проверкой при загрузке. Поэтому кэш кадров вынесен в
/// [CsmFrameCache] и подставляется вызывающей стороной.
library;

import 'dart:typed_data';

import 'package:caramba_vpn/csm.dart' show CsmHighWaterStore;

import 'package:caramba_client/data/models/csm_profile.dart';

/// Хранилище кадров последних принятых документов.
abstract class CsmFrameCache {
  Uint8List? read(int docType, String scope);
  void write(int docType, String scope, Uint8List frame);
}

/// Кэш в памяти. Годится для одного прогона проверки и для тестов; на диск
/// кадры кладёт вызывающая сторона.
class CsmMemoryFrameCache implements CsmFrameCache {
  final Map<String, Uint8List> _frames = <String, Uint8List>{};

  @override
  Uint8List? read(int docType, String scope) =>
      _frames[csmHighWaterKey(docType, scope)];

  @override
  void write(int docType, String scope, Uint8List frame) {
    _frames[csmHighWaterKey(docType, scope)] = frame;
  }
}

/// Отметки одного профиля поверх его карты `highWaterMarks`.
///
/// Реализует контракт проверяющего из пакета `caramba_vpn`, поэтому его можно
/// отдать `CsmTrustState` напрямую.
///
/// ХРАНИЛИЩЕ ОТМЕТОК РОВНО ОДНО, И ЭТО НЕ ОНО.
///
/// 02-SPEC.md 5.1: отметка максимума версий персистентна, монотонна и обязана
/// жить в ОДНОМ месте; два места это дыра для отката, а не защита в глубину, и
/// 01-DECISION.md X3 приводит конкретный случай, где два рабочих каталога дали
/// две отметки. Хранилищем записи назначен Go: `transport.Store` держит
/// `csm/<pid>/state.json`, отказывается понижать отметку и отдаёт её наружу в
/// `CsmStateJSON`. Этот класс это ПРОЕКЦИЯ для проверки на стороне Dart: он
/// принимает отметки, полученные от ядра, и его [advance] не имеет права стать
/// вторым писателем истины. Тест `csm_single_store_test.dart` падает, если
/// [advance] позовут из `lib/`.
class CsmProfileHighWaterStore implements CsmHighWaterStore {
  CsmProfileHighWaterStore({Map<String, int>? marks, CsmFrameCache? frames})
    : _marks = Map<String, int>.from(marks ?? const <String, int>{}),
      _frames = frames ?? CsmMemoryFrameCache();

  final Map<String, int> _marks;
  final CsmFrameCache _frames;

  /// Снимок отметок для записи обратно на профиль.
  Map<String, int> snapshot() => Map<String, int>.unmodifiable(_marks);

  @override
  int mark(int docType, String scope) =>
      _marks[csmHighWaterKey(docType, scope)] ?? 0;

  @override
  Uint8List? storedFrame(int docType, String scope) =>
      _frames.read(docType, scope);

  @override
  void advance(int docType, String scope, int version, Uint8List frame) {
    final key = csmHighWaterKey(docType, scope);
    final current = _marks[key] ?? 0;
    // Монотонность, и только вперёд: отметка не откатывается никогда, включая
    // случай, когда хранилище прочиталось пустым.
    if (version < current) {
      return;
    }
    _marks[key] = version;
    _frames.write(docType, scope, frame);
  }
}

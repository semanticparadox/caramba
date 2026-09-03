import 'dart:typed_data';

import 'package:caramba_vpn/src/csm/frame.dart';

// Таблица "тип документа -> роль -> порог" из 03-WIRE.md 7.1, как данные, а не
// как код. Это то самое правило, которое три реализации иначе изобрели бы
// каждая по-своему.
//
// Проверяющий ОБЯЗАН определить роль по doc_type в кадре и прочитать набор
// ключей и порог для этой роли из РАНЕЕ ДОВЕРЕННОГО ключевого документа. Он не
// имеет права читать их из проверяемого документа, кроме единственного
// исключения правила ротации 7.3, которое требует ОБА набора.

class CsmRoles {
  static const int root = 1;
  static const int online = 2;
  static const int timestamp = 3;

  static String name(int role) {
    switch (role) {
      case root:
        return 'root';
      case online:
        return 'online';
      case timestamp:
        return 'timestamp';
      default:
        return 'unknown';
    }
  }
}

/// Откуда берётся набор ключей для типа документа.
enum CsmKeySetSource {
  /// roles[N].ks ранее доверенного ключевого документа.
  trustedDocument,

  /// Объединение roles[1].ks доверенного документа и проверяемого: правило
  /// ротации корня 03-WIRE.md 7.3.
  trustedDocumentAndSelf,

  /// Единственный ключ, чей sha256[0..12] совпадает с link_pin (7.2).
  linkPin,
}

/// Строка таблицы 03-WIRE.md 7.1.
class CsmAuthorizationRow {
  const CsmAuthorizationRow(this.requiredRole, this.keySetSource);

  final int requiredRole;
  final CsmKeySetSource keySetSource;
}

/// Таблица целиком. У каждого типа, пережившего шаг P3, есть строка, поэтому
/// шаг V1 не может провалиться и не несёт кода ошибки.
const Map<int, CsmAuthorizationRow> csmAuthorizationTable =
    <int, CsmAuthorizationRow>{
  CsmDocType.keyDocument: CsmAuthorizationRow(
    CsmRoles.root,
    CsmKeySetSource.trustedDocumentAndSelf,
  ),
  CsmDocType.catalog: CsmAuthorizationRow(
    CsmRoles.online,
    CsmKeySetSource.trustedDocument,
  ),
  CsmDocType.directive: CsmAuthorizationRow(
    CsmRoles.online,
    CsmKeySetSource.trustedDocument,
  ),
  CsmDocType.catalogChunk: CsmAuthorizationRow(
    CsmRoles.online,
    CsmKeySetSource.trustedDocument,
  ),
  CsmDocType.bootstrapBlob: CsmAuthorizationRow(
    CsmRoles.root,
    CsmKeySetSource.linkPin,
  ),
  CsmDocType.sealedDirective: CsmAuthorizationRow(
    CsmRoles.online,
    CsmKeySetSource.trustedDocument,
  ),
  CsmDocType.reservePool: CsmAuthorizationRow(
    CsmRoles.root,
    CsmKeySetSource.trustedDocument,
  ),
};

/// Область действия отметки максимума версий (03-WIRE.md 6.3): локатор для
/// 0x03 и 0x08, cat_id для 0x02 и 0x04, пусто для 0x01 и 0x05.
///
/// V9 для 0x02 и 0x04 инертен по построению: cat_id выведен из собственных
/// байт каталога, поэтому два разных каталога никогда не делят область и любой
/// старый каталог сравнивается с пустой отметкой. Настоящая граница отката для
/// каталога это V14a плюс монотонность назвавшей его директивы.
String csmScopeFor(int docType, {String? locator, String? catId}) {
  switch (docType) {
    case CsmDocType.directive:
    case CsmDocType.reservePool:
      return locator ?? '';
    case CsmDocType.catalog:
    case CsmDocType.catalogChunk:
      return catId ?? '';
    default:
      return '';
  }
}

/// Хранилище отметок максимума версий. Оно ОБЯЗАНО быть ровно одно на профиль:
/// два рабочих каталога это две отметки, а это дыра для отката, а не защита в
/// глубину.
abstract class CsmHighWaterStore {
  int mark(int docType, String scope);
  Uint8List? storedFrame(int docType, String scope);
  void advance(int docType, String scope, int version, Uint8List frame);
}

/// Реализация в памяти, для тестов и для однократного прогона проверки.
class CsmMemoryHighWaterStore implements CsmHighWaterStore {
  CsmMemoryHighWaterStore();

  final Map<String, int> _marks = <String, int>{};
  final Map<String, Uint8List> _frames = <String, Uint8List>{};

  String _key(int docType, String scope) => '$docType|$scope';

  @override
  int mark(int docType, String scope) => _marks[_key(docType, scope)] ?? 0;

  @override
  Uint8List? storedFrame(int docType, String scope) =>
      _frames[_key(docType, scope)];

  @override
  void advance(int docType, String scope, int version, Uint8List frame) {
    final k = _key(docType, scope);
    final current = _marks[k] ?? 0;
    if (version < current) {
      return;
    }
    _marks[k] = version;
    _frames[k] = frame;
  }
}

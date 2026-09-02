/// Текст ошибки ядра для показа пользователю.
///
/// Ядро возвращает свои ошибки по-разному в зависимости от транспорта:
/// канальный путь бросает [PlatformException] (текст в `message`), FFI-путь —
/// `CarambaCoreException` (текст в `message`, но `toString` добавляет префикс).
/// UI должен показывать ИМЕННО текст ядра — он объясняет, что не так с
/// подпиской, лучше любой нашей формулировки.
library;

import 'package:caramba_vpn/caramba_vpn.dart' show CarambaCoreException;
import 'package:flutter/services.dart' show PlatformException;

/// Достаёт человекочитаемый текст из ошибки ядра. Для неизвестных исключений
/// возвращает `null`: вызывающий подставит свою формулировку.
String? coreErrorText(Object error) {
  if (error is CarambaCoreException) {
    return error.message.trim().isEmpty ? null : error.message.trim();
  }
  if (error is PlatformException) {
    final message = error.message?.trim();
    if (message != null && message.isNotEmpty) return message;
    final details = error.details?.toString().trim();
    if (details != null && details.isNotEmpty) return details;
    return error.code.isEmpty ? null : error.code;
  }
  return null;
}

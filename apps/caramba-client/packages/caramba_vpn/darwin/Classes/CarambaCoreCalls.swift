// CarambaCoreCalls — мост между тем, как gomobile объявляет ошибки, и тем, как
// их ждёт остальной Swift-код плагина.
//
// Go-метод вида `func (c *Client) CsmState() (string, error)` gobind выносит в
// Objective-C как
//
//     - (NSString* _Nonnull)csmState:(NSError* _Nullable* _Nullable)error;
//
// Возврат помечен _Nonnull, а импортёр Objective-C переводит хвостовой
// NSError** в `throws` ТОЛЬКО когда об ошибке есть чем сигналить: BOOL или
// nullable-объект. Ненулевая строка таким сигналом не является, поэтому в Swift
// метод приезжает как `csmState(_ error: NSErrorPointer) -> String` и `try` к
// нему неприменим. Методы, возвращающие BOOL (`down`, `configure`, `setTunFd`,
// …), наоборот, приезжают обычными throws — их обёртка не касается.
//
// Без обёртки два десятка вызовов ядра завели бы каждый свой `var err: NSError?`
// и свою проверку, и одна забытая проверка означала бы «ошибка ядра прочитана
// как пустой JSON». Здесь она ровно одна.
//
// Файл сознательно не импортирует Flutter: его компилирует и цель Network
// Extension (см. Extension/README.md), где Flutter'а нет.

import Foundation

#if CARAMBA_CORE

/// Вызывает строковый метод ядра и превращает записанный им NSError в throw.
///
/// - Parameter body: замыкание, отдающее указатель ошибки в метод ядра.
/// - Returns: строку, которую вернуло ядро.
/// - Throws: NSError, записанный ядром.
func carambaCoreCall(_ body: (NSErrorPointer) -> String) throws -> String {
    var error: NSError?
    let out = body(&error)
    if let error = error { throw error }
    return out
}

/// Тот же вызов, но «ошибка — это просто нет ответа»: для мест, где предыдущее
/// значение лучше исключения (опрос 1 Гц, отчёт о маршрутизации).
func carambaCoreTry(_ body: (NSErrorPointer) -> String) -> String? {
    var error: NSError?
    let out = body(&error)
    if error != nil { return nil }
    return out
}

#endif

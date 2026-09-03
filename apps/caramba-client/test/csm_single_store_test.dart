// 02-SPEC.md 5.1: отметка максимума версий и временной пол живут в ОДНОМ
// хранилище. Хранилище записи это Go (`transport.Store`, csm/<pid>/state.json);
// сторона Dart держит проекцию для проверки и НЕ имеет права стать вторым
// писателем. Два хранилища это дыра для отката: документ, отвергнутый одной
// отметкой, будет принят против другой.
//
// Тест структурный намеренно. Поймать это поведением нельзя, пока второй
// писатель ещё не появился, а появиться он может одной строкой в любом файле.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('в lib/ нет второго писателя отметок версий', () {
    final lib = Directory('lib');
    expect(lib.existsSync(), isTrue, reason: 'тест запускается из корня пакета');

    final offenders = <String>[];
    for (final f in lib.listSync(recursive: true).whereType<File>()) {
      if (!f.path.endsWith('.dart')) continue;
      final src = f.readAsStringSync();
      // Продвижение отметки и запись пола: ровно те две операции, которые
      // делают хранилище хранилищем.
      if (RegExp(r'\.advance\(').hasMatch(src)) {
        offenders.add('${f.path}: вызывает CsmHighWaterStore.advance');
      }
    }
    expect(
      offenders,
      isEmpty,
      reason:
          'Отметки версий продвигает ядро на Go и только оно. Появился второй '
          'писатель:\n${offenders.join('\n')}',
    );
  });
}

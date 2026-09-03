/// Судьба записи настроек на экране настроек.
///
/// Нормативно: 02-SPEC.md 7.8 (изменение настроек не блокируется на сети,
/// очередь переживает отказ), 6.2 (контрол за снятым битом не рисуется рабочим),
/// INV-16 (сетевой отказ ничего не откатывает).
///
/// Экран существует потому, что четыре исхода отдачи очереди раньше не доходили
/// ни до одного виджета. Пользователь менял настройку, локальное значение
/// применялось, запись молча не уходила, и на экране не было ни строки об этом.
/// Два случая различаются и различаются здесь: `notOffered` это ПОСТОЯННОЕ
/// свойство оператора, который записи настроек не предлагает вовсе, а `failed`
/// это временный отказ сети, после которого запись уйдёт сама.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class CsmWriteStatusNote extends ConsumerWidget {
  const CsmWriteStatusNote({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(csmWriteStatusProvider);
    if (!status.isNoteworthy) {
      return const SizedBox.shrink();
    }
    final text = status.isPermanent
        ? 'Этот оператор не принимает изменения настроек от приложения. '
              'Значения применены на устройстве и останутся здесь, но у '
              'оператора они не появятся.'
        : 'Изменение применено на устройстве, но оператору ещё не доставлено. '
              'Оно уйдёт само, как только будет связь: очередь не теряется и '
              'значение не откатывается.';
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSpace.s4),
      child: InlineBanner(
        tone: status.isPermanent ? BannerTone.info : BannerTone.warning,
        glyph: status.isPermanent ? Lucide.alert : Lucide.clock,
        text: text,
      ),
    );
  }
}

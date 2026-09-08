/// Строка десктопной формы настроек: имя и объяснение слева, контрол справа.
///
/// Почему не [CRow]. Мобильная строка кладёт имя и ЗНАЧЕНИЕ в одну линию и
/// открывает лист по тапу: значение там показано текстом, потому что менять
/// его всё равно уходят на другой экран. На десктопе менять значение можно
/// прямо здесь ([DesktopPicker], `Switch`), и колонка значения превращается в
/// колонку контролов — правая граница у всех строк раздела обязана совпадать,
/// иначе форма читается как список разной длины строк. Поэтому ширину
/// контрола задаёт САМ контрол (пикер 240, кнопка 120, `Switch` своей
/// естественной), а строка только выравнивает его по правому краю.
///
/// Описание живёт ПОД именем и переносится, а не обрезается: здесь стоят
/// причины отказа ядра (подпись блока рекламы) и сводки списков, а обрезанная
/// причина хуже отсутствующей. Тот же выбор уже сделан в мобильных настройках
/// (`_StackedRow`) и в [AppliedRouteCard].
library;

import 'package:flutter/material.dart';

import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// Минимальная высота строки формы: 44 это тот же минимум цели нажатия, что и
/// у мобильных строк, только без их вертикального воздуха.
const double kFormRowMinHeight = 44;

/// Минимальная ширина кнопки «Открыть»/«Выбрать»/«Изменить».
const double kFormButtonWidth = 120;

/// Ширина пикера в колонке контролов. Совпадает с шириной поля [DesktopPicker]
/// по умолчанию — правая граница строк раздела обязана быть одной.
const double kFormPickerWidth = 240;

class FormRow extends StatelessWidget {
  /// Имя настройки. То же слово, что и в мобильных настройках: расхождение
  /// между платформами читается как две разные настройки.
  final String label;

  /// Что это значит или что сейчас выбрано. Переносится на любое число строк.
  final String? description;

  /// Метка происхождения значения (`CsmProvenanceTag`) между текстом и
  /// контролом. `null` — профиль без CSM, происхождения не существует.
  final Widget? provenance;

  /// Контрол справа: `Switch`, [DesktopPicker] или [FormOpenButton].
  final Widget control;

  /// Нажатие по всей строке. Задаётся только там, где строка ВЕДЁТ куда-то
  /// целиком; у строки с переключателем его нет — иначе одно нажатие означало
  /// бы два разных действия в зависимости от того, куда попал курсор.
  final VoidCallback? onTap;

  const FormRow({
    required this.label,
    required this.control,
    this.description,
    this.provenance,
    this.onTap,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final desc = description;

    final row = Container(
      constraints: const BoxConstraints(minHeight: kFormRowMinHeight),
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpace.s4,
        vertical: AppSpace.s3,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(label, style: AppType.bodyMd.copyWith(color: c.textHi)),
                if (desc != null && desc.isNotEmpty) ...[
                  const SizedBox(height: 2),
                  Text(desc, style: AppType.bodySm.copyWith(color: c.textMed)),
                ],
              ],
            ),
          ),
          const SizedBox(width: AppSpace.s3),
          if (provenance != null) provenance!,
          control,
        ],
      ),
    );

    if (onTap == null) return row;
    return InkWell(onTap: onTap, child: row);
  }
}

/// Кнопка «уйти отсюда за значением»: подэкран, лист или панель.
///
/// Обычная ширина — [kFormButtonWidth]; увеличенный системный шрифт может
/// расширить кнопку, чтобы подпись оставалась целой. Высота 36 —
/// десктопная плотность, мобильные 50 в форме выглядят кнопками действия, а не
/// строкой настройки.
class FormOpenButton extends StatelessWidget {
  /// Что произойдёт: «Открыть» (экран), «Выбрать» (список), «Изменить» (лист).
  final String label;
  final VoidCallback? onPressed;

  const FormOpenButton({required this.label, this.onPressed, super.key});

  @override
  Widget build(BuildContext context) {
    return IntrinsicWidth(
      child: OutlinedButton(
        onPressed: onPressed,
        style: OutlinedButton.styleFrom(
          minimumSize: const Size(kFormButtonWidth, 36),
          padding: const EdgeInsets.symmetric(horizontal: AppSpace.s3),
        ),
        child: Text(label, maxLines: 1, softWrap: false),
      ),
    );
  }
}

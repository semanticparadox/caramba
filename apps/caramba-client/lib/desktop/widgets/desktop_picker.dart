/// Выбор значения из закрытого списка на десктопе: поле с текущим значением и
/// меню под ним.
///
/// ПОЧЕМУ НЕ ЛИСТ. `showPickerSheet` поднимает нижний лист на пол-экрана —
/// на телефоне это вся доступная площадь, а на десктопе это модальный слой
/// поверх окна ради выбора между «Авто» и «gVisor». Значение здесь меняют
/// сериями (стек, DNS, MTU подряд), и лист на каждое значение стоит человеку
/// анимации входа, потери места на экране и поиска, куда делась форма.
/// [MenuAnchor] открывается у самого поля и закрывается Esc, кликом снаружи и
/// выбором.
///
/// ЧТО СОХРАНЕНО ОТ ЛИСТА, дословно:
///   * та же форма опции — `(name, desc, icon)`, тот же тип, что у
///     `showPickerSheet`, чтобы списки (`CoreOption.stacks` и прочие) не
///     переписывались под вторую платформу;
///   * недоступное значение ВИДНО и выключено с названной причиной
///     (02-SPEC.md 7.2 и 7.9), а не исчезает из списка: пропавшая строка
///     неотличима от «такого не бывает», и человек идёт искать её в
///     обновлении, которого ему не нужно;
///   * та же прозрачность 0.45 у выключенных строк, что в листе.
///
/// `icon` в опции ПРИНИМАЕТСЯ и не рисуется: у меню шириной 280 глиф слева
/// съедает колонку, в которой живёт описание. Поле оставлено в сигнатуре
/// ровно ради совместимости со списками общего пикера.
library;

import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Высота поля. Меньше строки формы (44): поле стоит ВНУТРИ строки.
const double kDesktopPickerFieldHeight = 36;

/// Минимальная ширина меню. Описание опции — целая фраза («TCP через system,
/// UDP через gVisor»), и в ширину поля 240 она разваливается на четыре строки.
const double kDesktopPickerMenuMinWidth = 280;

/// Ширина колонки под галочку выбранного значения.
const double _checkColumn = 22;

class DesktopPicker extends StatelessWidget {
  /// Опции в том же виде, в каком их принимает `showPickerSheet`.
  final List<({String name, String desc, String? icon})> options;

  /// Индекс текущего значения в [options].
  final int selected;

  /// Индексы, которые видны, но не выбираемы, и причина к каждому.
  final Map<int, String> disabled;

  final ValueChanged<int> onSelected;

  /// Ширина ПОЛЯ (не меню): правая граница строк раздела обязана быть одной.
  final double width;

  const DesktopPicker({
    required this.options,
    required this.selected,
    required this.onSelected,
    this.disabled = const <int, String>{},
    this.width = 240,
    super.key,
  });

  /// Имя текущего значения. Индекс вне списка это запись чужой версии, а не
  /// повод показать пустое поле: показываем первое значение, как это делает
  /// мобильный экран настроек.
  String get _valueName {
    if (options.isEmpty) return '';
    final i = (selected >= 0 && selected < options.length) ? selected : 0;
    return options[i].name;
  }

  /// Куда уйдёт фокус при открытии меню.
  ///
  /// Меню без фокуса внутри не слышит Esc: `DismissIntent` ловит `Actions`
  /// панели меню, а до неё событие доходит только по цепочке фокуса. Заодно
  /// это правильное поведение клавиатуры: меню открывается на ТЕКУЩЕМ
  /// значении, а не на первой строке.
  int? get _autofocusIndex {
    if (disabled[selected] == null &&
        selected >= 0 &&
        selected < options.length) {
      return selected;
    }
    for (var i = 0; i < options.length; i++) {
      if (disabled[i] == null) return i;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final focusIndex = _autofocusIndex;

    return MenuAnchor(
      // Клик мимо меню закрывает его и НЕ доезжает до строки под ним: иначе
      // закрытие меню переключало бы соседний тумблер.
      consumeOutsideTap: true,
      style: const MenuStyle(
        // Поверхность, рамка и тень рисуются нашим контейнером внутри:
        // `MenuStyle` умеет только `elevation`, а тень у нас токен
        // (`AppShadows.raised`), и подменять её материаловской высотой
        // значило бы завести второй источник теней в приложении.
        backgroundColor: WidgetStatePropertyAll<Color>(Colors.transparent),
        surfaceTintColor: WidgetStatePropertyAll<Color>(Colors.transparent),
        shadowColor: WidgetStatePropertyAll<Color>(Colors.transparent),
        elevation: WidgetStatePropertyAll<double>(0),
        padding: WidgetStatePropertyAll<EdgeInsetsGeometry>(EdgeInsets.zero),
        alignment: Alignment.bottomLeft,
      ),
      menuChildren: <Widget>[
        Container(
          constraints: BoxConstraints(
            minWidth: math.max(kDesktopPickerMenuMinWidth, width),
          ),
          decoration: BoxDecoration(
            color: c.surface3,
            borderRadius: AppRadius.r12,
            border: Border.all(color: c.borderSubtle),
            boxShadow: context.tokens.elevRaised,
          ),
          padding: const EdgeInsets.symmetric(vertical: AppSpace.s1),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: <Widget>[
              for (var i = 0; i < options.length; i++)
                _item(context, i, autofocus: i == focusIndex),
            ],
          ),
        ),
      ],
      builder: (context, controller, child) {
        return Semantics(
          button: true,
          value: _valueName,
          child: SizedBox(
            width: width,
            height: kDesktopPickerFieldHeight,
            child: Material(
              color: c.surface2,
              borderRadius: AppRadius.r12,
              child: InkWell(
                onTap: () =>
                    controller.isOpen ? controller.close() : controller.open(),
                borderRadius: AppRadius.r12,
                child: Container(
                  decoration: BoxDecoration(
                    borderRadius: AppRadius.r12,
                    border: Border.all(
                      color: c.borderStrong,
                      width: AppBorders.input,
                    ),
                  ),
                  padding: const EdgeInsets.symmetric(horizontal: AppSpace.s3),
                  child: Row(
                    children: <Widget>[
                      Expanded(
                        child: Text(
                          _valueName,
                          overflow: TextOverflow.ellipsis,
                          style: AppType.bodyMd.copyWith(color: c.textHi),
                        ),
                      ),
                      const SizedBox(width: AppSpace.s2),
                      LucideIcon(
                        Lucide.chevronDown,
                        color: c.textLow,
                        size: 16,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  Widget _item(BuildContext context, int i, {required bool autofocus}) {
    final c = context.c;
    final option = options[i];
    final off = disabled[i];

    final body = Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        SizedBox(
          width: _checkColumn,
          child: off == null && i == selected
              ? Padding(
                  padding: const EdgeInsets.only(top: 3),
                  child: LucideIcon(Lucide.check, color: c.textHi, size: 16),
                )
              : null,
        ),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: <Widget>[
              Text(
                option.name,
                style: AppType.bodyMd.copyWith(color: c.textHi),
              ),
              // Причина недоступности ЗАМЕЩАЕТ описание, а не приписывается к
              // нему: человеку, который не может выбрать значение, сначала
              // нужно знать почему.
              if ((off ?? option.desc).isNotEmpty) ...[
                const SizedBox(height: 2),
                Text(
                  off ?? option.desc,
                  style: AppType.bodySm.copyWith(color: c.textMed),
                ),
              ],
            ],
          ),
        ),
      ],
    );

    return MenuItemButton(
      autofocus: autofocus,
      onPressed: off == null ? () => onSelected(i) : null,
      style: const ButtonStyle(
        padding: WidgetStatePropertyAll<EdgeInsetsGeometry>(
          EdgeInsets.symmetric(horizontal: AppSpace.s3, vertical: AppSpace.s2),
        ),
        minimumSize: WidgetStatePropertyAll<Size>(Size(0, 40)),
      ),
      child: off == null ? body : Opacity(opacity: 0.45, child: body),
    );
  }
}

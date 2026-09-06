/// Кнопка автоподбора на Главной.
///
/// Жалоба владельца дословно: «кнопка автоподбор не должна выглядеть как
/// остальные выпадающие списки, она должна быть отдельной кнопкой другим
/// стилем». Она была права по существу, а не по вкусу: автоподбор — ДЕЙСТВИЕ
/// («замерь узлы и выбери лучший»), а «Сервер», «Relay», «Тип подключения» и
/// «Режим» — ВЫБОРЫ из списка. Пока автоподбор стоял [CRow]-строкой с шевроном
/// внутри той же `RowsGroup`, форма обещала пятый список, а за шевроном
/// открывался замер. Поэтому строка ушла из группы, а не просто поменяла цвет.
///
/// Кнопка живёт своим файлом, а не в `ui.dart`: там общие компоненты всего
/// приложения, а здесь — виджет с пятью состояниями автоподбора, который никому
/// больше не нужен. Геометрия при этом ровно та же, что у [GhostButton]
/// (высота 50, `AppRadius.r14`, обводка + surface2) — она приходит из темы
/// `outlinedButtonTheme`, так что кнопка не выпадает из набора, отличаясь от
/// строк группы формой, а не самодельными числами.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// В каком из пяти состояний кнопка.
enum AutopilotButtonState {
  /// Подбор ни разу не запускали на этом профиле.
  neverRun,

  /// Замер идёт прямо сейчас.
  running,

  /// Выбор есть и он исполняется.
  inForce,

  /// Выбор есть, но трафик идёт (или пойдёт) мимо него.
  notInForce,

  /// Выбор есть, но описывает не сегодняшнюю действительность.
  stale,
}

/// Слово состояния на самой кнопке.
///
/// Словарь общий с экраном «Серверы» и с баннером Главной: «не в силе» — выбор
/// не исполняется (замер при этом свежий), «устарело» — устарел сам замер.
/// Разные слова здесь не украшение: они ведут человека в разные места. «Не в
/// силе» лечится переподключением, «устарело» — новым замером, и перепутать их
/// значит послать перезамерять там, где перезамер ничего не изменит.
const String kAutopilotBadgeInForce = 'в силе';
const String kAutopilotBadgeNotInForce = 'не в силе';
const String kAutopilotBadgeStale = 'устарело';

/// Что кнопка говорит и в каком она состоянии — без единого виджета.
///
/// Отделено от отрисовки намеренно: правило «какое слово при какой причине»
/// проверяется таблицей на все причины [AutoStaleReason] разом, а не пятью
/// прогонами дерева виджетов, где ошибку в одной ветке легко не заметить.
class AutopilotButtonModel {
  final AutopilotButtonState state;

  /// Надпись на кнопке.
  final String text;

  /// Слово состояния справа; пусто — состояние без слова.
  final String badge;

  const AutopilotButtonModel({
    required this.state,
    required this.text,
    this.badge = '',
  });
}

/// Как назвать выбор автоподбора одной строкой.
///
/// Правило то же, что у подписи строки «Сервер» (`autoServerLabelProvider`):
/// «CA · Канада», а при пустом или совпадающем коде страны — просто имя. Две
/// разные записи одного и того же выбора на одном экране человек читает как два
/// разных узла, поэтому правило здесь повторено, а не придумано заново.
String autopilotChoiceLabel(AutoPickRecord pick) {
  final title = pick.shortLabel;
  final cc = pick.countryCode;
  return (cc.isEmpty || cc == title) ? title : '$cc · $title';
}

/// Чистая половина кнопки: состояние, надпись и слово состояния.
AutopilotButtonModel autopilotButtonModel({
  required bool running,
  required AutoPickRecord? pick,
  required AutoStaleReason stale,
}) {
  // Идущий замер важнее прошлого выбора: прошлый выбор сейчас перерешается, и
  // объявлять его «в силе» в этот момент значило бы обещать то, что через
  // секунду сменится.
  if (running) {
    return const AutopilotButtonModel(
      state: AutopilotButtonState.running,
      text: 'Подбираю узел…',
    );
  }
  if (pick == null) {
    return const AutopilotButtonModel(
      state: AutopilotButtonState.neverRun,
      text: 'Подобрать лучший узел',
    );
  }
  final label = 'Автоподбор: ${autopilotChoiceLabel(pick)}';
  return switch (stale) {
    AutoStaleReason.none => AutopilotButtonModel(
      state: AutopilotButtonState.inForce,
      text: label,
      badge: kAutopilotBadgeInForce,
    ),
    // Замер свежий — не в силе сам ВЫБОР: держатель другой.
    AutoStaleReason.tunnelDisagrees ||
    AutoStaleReason.pinDisagrees => AutopilotButtonModel(
      state: AutopilotButtonState.notInForce,
      text: label,
      badge: kAutopilotBadgeNotInForce,
    ),
    AutoStaleReason.age || AutoStaleReason.fleetChanged => AutopilotButtonModel(
      state: AutopilotButtonState.stale,
      text: label,
      badge: kAutopilotBadgeStale,
    ),
  };
}

/// Отдельная кнопка автоподбора под группой строк Главной.
class AutopilotButton extends ConsumerWidget {
  const AutopilotButton({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    // `autopilotProvider` — уровня приложения, и ход замера переживает уход с
    // экрана автонастройки: значит состояние «идёт» с Главной ДОСТИЖИМО и
    // эмулировать его нечем и незачем.
    final model = autopilotButtonModel(
      running: ref.watch(autopilotProvider).running,
      pick: ref.watch(autoPickRecordProvider),
      stale: ref.watch(autoStaleProvider),
    );

    final Color border = switch (model.state) {
      AutopilotButtonState.inForce => c.accent,
      AutopilotButtonState.notInForce => c.warning,
      _ => c.borderSubtle,
    };
    final Color badgeColor = switch (model.state) {
      AutopilotButtonState.inForce => c.success,
      AutopilotButtonState.notInForce => c.warning,
      _ => c.textMed,
    };

    return SizedBox(
      width: double.infinity,
      child: OutlinedButton(
        // Кнопка остаётся нажимаемой и во время замера. Двойного запуска этим
        // не вызвать (`AutopilotController.run` отбивает повторный вызов сам),
        // а ведёт она на единственный экран, где ход замера видно, — запереть
        // вход туда ровно в ту секунду, когда кнопка объявляет «Подбираю
        // узел…», значило бы спрятать то, о чём она сообщает.
        onPressed: () => context.go(AppRoute.settingsAutotune),
        style: OutlinedButton.styleFrom(
          side: BorderSide(color: border, width: AppBorders.hairline),
          // Текст на кнопке длинный и выровнен по левому краю, поэтому поля
          // задаются здесь, а не наследуются от центрированной ghost-кнопки.
          padding: const EdgeInsets.symmetric(horizontal: AppSpace.s4),
        ),
        child: Row(
          children: [
            if (model.state == AutopilotButtonState.running)
              SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  valueColor: AlwaysStoppedAnimation<Color>(c.textMed),
                ),
              )
            else
              LucideIcon(
                model.state == AutopilotButtonState.inForce
                    ? Lucide.check
                    : Lucide.gauge,
                color: model.state == AutopilotButtonState.inForce
                    ? c.success
                    : c.textMed,
                size: 18,
              ),
            const SizedBox(width: AppSpace.s3),
            Expanded(
              child: Text(
                model.text,
                overflow: TextOverflow.ellipsis,
                style: AppType.label.copyWith(color: c.textHi),
              ),
            ),
            if (model.badge.isNotEmpty) ...[
              const SizedBox(width: AppSpace.s2),
              _StateTag(model.badge, color: badgeColor),
            ],
          ],
        ),
      ),
    );
  }
}

/// Слово состояния в рамке — [Tag] с произвольным тоном.
///
/// Общий [Tag] знает два тона (нейтральный и `ok`), а состояний у кнопки три, и
/// «не в силе» обязано быть предупреждением, а не серой подписью: это
/// расхождение выбора с действительностью, а не справка. Дописать третий тон в
/// `ui.dart` нельзя — он общий на всё приложение и в этот круг правок не
/// входит, поэтому геометрия [Tag] повторена здесь один в один.
class _StateTag extends StatelessWidget {
  final String text;
  final Color color;

  const _StateTag(this.text, {required this.color});

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(5),
        border: Border.all(color: color),
      ),
      child: Text(
        text.toUpperCase(),
        style: AppType.monoSm.copyWith(color: color, letterSpacing: 0.6),
      ),
    );
  }
}

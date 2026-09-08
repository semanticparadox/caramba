// Список выходов: подпись «Авто» не должна удваивать точку на стыке.
//
// `_autoSubtitle` склеивает два предложения — то, что уже выбрал автопилот, и
// подсказку «В пределах страны … снять закрепление» — и `chosen` сам иногда
// уже кончается точкой (устаревший выбор называется через
// `AutoLabel._staleText`, а он — законченное предложение). Слепая склейка
// `'$chosen. $tail'` в этом случае превращала одну точку в две.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/offering_builder.dart';
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/servers/exit_node_list.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/theme/app_theme.dart';

Widget _host({required ExitInventory inventory, required AutoLabel auto}) =>
    ProviderScope(
      overrides: [
        exitInventoryProvider.overrideWithValue(inventory),
        offeringProvider.overrideWithValue(buildEmptyOffering()),
        autoServerLabelProvider.overrideWithValue(auto),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(),
        home: Scaffold(body: ExitNodeList(onSelect: (_) {})),
      ),
    );

void main() {
  testWidgets(
    'подпись «Авто» не удваивает точку, когда выбор устарел И страна закреплена',
    (tester) async {
      const inventory = ExitInventory(
        source: ExitInventorySource.panelRest,
        nodes: [
          ExitNode(
            key: 'n1',
            name: 'DE-1',
            countryCode: 'DE',
            source: ExitInventorySource.panelRest,
          ),
        ],
        // Страна закреплена — «Авто» дописывает подсказку про снятие пина,
        // и это ровно тот хвост, что раньше слепо клеился второй точкой.
        selectedCountry: 'DE',
      );
      // `stale: tunnelDisagrees` заставляет AutoLabel.subtitle отдать
      // `_staleText` — готовое предложение, УЖЕ кончающееся точкой.
      const auto = AutoLabel(
        choice: 'Германия',
        source: 'сейчас в туннеле',
        stale: AutoStaleReason.tunnelDisagrees,
      );
      expect(auto.subtitle, endsWith('переподключении.'));

      await tester.pumpWidget(_host(inventory: inventory, auto: auto));
      await tester.pump();

      final subtitle = tester
          .widgetList<Text>(find.textContaining('Пересчитается'))
          .single
          .data!;

      expect(subtitle, isNot(contains('..')));
      expect(
        subtitle,
        'Ядро сейчас стоит на другом узле. Пересчитается при '
        'переподключении. В пределах страны: DE. Нажмите, чтобы снять '
        'закрепление.',
      );
    },
  );

  testWidgets(
    'подпись «Авто» ставит точку, когда выбор ещё свежий (без готового предложения)',
    (tester) async {
      const inventory = ExitInventory(
        source: ExitInventorySource.panelRest,
        nodes: [
          ExitNode(
            key: 'n1',
            name: 'DE-1',
            countryCode: 'DE',
            source: ExitInventorySource.panelRest,
          ),
        ],
        selectedCountry: 'DE',
      );
      // Без stale подпись — просто `source`, без своей точки: обычный, не
      // вырожденный случай склейки должен по-прежнему получать точку.
      const auto = AutoLabel(choice: 'Германия', source: 'сейчас в туннеле');
      expect(auto.subtitle, isNot(endsWith('.')));

      await tester.pumpWidget(_host(inventory: inventory, auto: auto));
      await tester.pump();

      final subtitle = tester
          .widgetList<Text>(find.textContaining('В пределах страны'))
          .single
          .data!;

      expect(subtitle, isNot(contains('..')));
      expect(
        subtitle,
        'сейчас в туннеле. В пределах страны: DE. Нажмите, чтобы снять '
        'закрепление.',
      );
    },
  );
}

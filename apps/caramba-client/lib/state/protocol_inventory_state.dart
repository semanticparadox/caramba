import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/offering/availability.dart'
    show OfferingStatus;
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Инвентарь протоколов: какие из протоколов, которые ядро умеет ПОПРОСИТЬ,
/// текущий источник действительно раздаёт — и что он раздаёт сверх них.
///
/// [ProtocolOption.defaults] это список запросов, а не список фактов. Экран,
/// собранный прямо по нему, обещает пользователю AmneziaWG на флоте, где нет ни
/// одного wireguard-узла: выбор применяется, ядро не находит подходящего
/// outbound'а и тихо остаётся на прежнем. Молчаливая деградация — худший исход
/// из возможных: пользователь уверен, что сменил протокол, и объясняет
/// оставшуюся блокировку чем угодно, кроме неё.
///
/// Здесь тот же дом-правило, что и у стран: недоступный протокол ВИДЕН и
/// назван с причиной. Причём в обе стороны — протокол, который источник
/// раздаёт, а ядро попросить не умеет (`naive` на первом узле флота), тоже
/// строка в списке, а не пустое место.

/// Почему протокол нельзя выбрать (или нельзя проверить).
enum ProtocolUnavailableReason {
  /// Источник перечислил протоколы, и этого среди них нет.
  notInFleet,

  /// Протокол есть, но не в той форме: `vless` без `reality` это не Reality.
  shapeNotInFleet,

  /// Источник раздаёт этот протокол, но у ядра нет строки, которой его
  /// попросить: `Policy.Protocol` знает закрытый набор имён.
  notRequestable,

  /// Профиль подключения не выбран — перечислять нечего.
  noProfile,

  /// В источнике нет ни одного узла.
  sourceEmpty,

  /// Источник узлы даёт, а протоколы по ним не сообщает (панельный
  /// `GET /servers` отдаёт имя, страну и пинг, но не тип outbound'а).
  sourceSilent,
}

/// Доступность протокола: доступен, недоступен с причиной — или НЕИЗВЕСТЕН с
/// причиной.
///
/// Третье состояние появилось потому, что второго не хватало. Молчащий источник
/// раньше давал `available` на каждую строку («источник ничего не исключил —
/// значит всё разрешено»), и приложение обещало AmneziaWG на флоте без единого
/// wireguard-узла. Обратное решение не лучше: `unavailable` по молчанию
/// выключило бы рабочие протоколы. Молчание — это [OfferingStatus.unknown]:
/// строка остаётся нажимаемой, но помечена как непроверенная.
class ProtocolAvailability {
  final OfferingStatus status;

  /// `null` — доступен.
  final ProtocolUnavailableReason? reason;

  /// Уточнение (форма, которой не хватило; имя источника). Может быть `null`.
  final String? detail;

  const ProtocolAvailability._(this.status, this.reason, this.detail);

  static const ProtocolAvailability available = ProtocolAvailability._(
    OfferingStatus.available,
    null,
    null,
  );

  const ProtocolAvailability.unavailable(
    ProtocolUnavailableReason reason, {
    String? detail,
  }) : this._(OfferingStatus.unavailable, reason, detail);

  /// Источник про эту строку ничего не сказал.
  const ProtocolAvailability.unknown(
    ProtocolUnavailableReason reason, {
    String? detail,
  }) : this._(OfferingStatus.unknown, reason, detail);

  /// Можно ли нажать. «Неизвестно» — можно: запрет по молчанию источника отнял
  /// бы рабочий выбор ровно так же, как разрешение по молчанию его выдумывало.
  bool get isAvailable => status != OfferingStatus.unavailable;

  /// Подтвердил ли источник это утверждение.
  bool get isVerified => status != OfferingStatus.unknown;

  bool get isUnknown => status == OfferingStatus.unknown;

  /// Готовый текст причины. Он живёт рядом с самой причиной, чтобы «недоступно»
  /// нигде не осталось без ответа на вопрос «почему».
  String get message {
    switch (reason) {
      case null:
        return 'Доступно';
      case ProtocolUnavailableReason.notInFleet:
        return 'Ни один узел источника не раздаёт этот протокол.';
      case ProtocolUnavailableReason.shapeNotInFleet:
        return detail == null
            ? 'Протокол есть, но не в этой форме.'
            : 'Протокол есть, но узлов с формой «$detail» среди них нет.';
      case ProtocolUnavailableReason.notRequestable:
        return 'Источник его раздаёт, но приложение не умеет его запросить.';
      case ProtocolUnavailableReason.noProfile:
        return 'Профиль подключения не выбран.';
      case ProtocolUnavailableReason.sourceEmpty:
        return 'В источнике нет ни одного узла.';
      case ProtocolUnavailableReason.sourceSilent:
        return 'Источник не сообщает протоколы узлов, проверить список не по '
            'чему.';
    }
  }

  @override
  bool operator ==(Object other) =>
      other is ProtocolAvailability &&
      other.status == status &&
      other.reason == reason &&
      other.detail == detail;

  @override
  int get hashCode => Object.hash(status, reason, detail);
}

/// Одна строка экрана протоколов.
class ProtocolChoice {
  /// Индекс в [protocolsProvider] — то, что уходит в `CoreConfig.protocol`.
  /// `-1` — протокол существует только в инвентаре, попросить его нечем.
  final int index;

  final String name;
  final String desc;
  final String icon;
  final bool recommended;
  final bool auto;

  /// Формы, в которых источник раздаёт этот протокол (`ws`, `grpc`, `reality`,
  /// `tcp`, ...). Пусто — источник форм не называет.
  final List<String> shapes;

  /// На скольких узлах источника он встретился. `0` при [sourceSilent] значит
  /// «неизвестно», а не «нет»: строка тогда доступна.
  final int nodeCount;

  final ProtocolAvailability availability;

  const ProtocolChoice({
    required this.index,
    required this.name,
    required this.desc,
    required this.icon,
    this.recommended = false,
    this.auto = false,
    this.shapes = const <String>[],
    this.nodeCount = 0,
    this.availability = ProtocolAvailability.available,
  });

  bool get isAvailable => availability.isAvailable;

  /// У ядра есть строка, которой этот протокол просят.
  bool get isRequestable => index >= 0;
}

/// Снимок инвентаря протоколов.
class ProtocolInventory {
  final ExitInventorySource source;

  /// Строки в порядке [protocolsProvider]; протоколы, которых у ядра нет,
  /// дописаны в конец.
  final List<ProtocolChoice> choices;

  /// Источник перечислил протоколы, и список сверен с ним.
  final bool verified;

  /// Почему сверить не удалось; `null` — сверено.
  final ProtocolAvailability? unverified;

  /// Узел, по которому список отфильтрован. `null` — узел не закреплён, и
  /// список описывает ВЕСЬ флот.
  ///
  /// Правило владельца: «протокол это и есть выбор inbounds, доступных в
  /// конфиге, а сервер должен быть выбор из нод» — то есть протоколы
  /// принадлежат конкретной машине. Пока узел не выбран, честный ответ это «что
  /// бывает во флоте», и он обязан быть назван именно так, а не выдан за «что
  /// применится».
  final String? scopedNodeKey;

  const ProtocolInventory({
    required this.source,
    required this.choices,
    required this.verified,
    this.unverified,
    this.scopedNodeKey,
  });

  /// Список относится к одному узлу, а не ко всему флоту.
  bool get isScopedToNode => scopedNodeKey != null;

  /// Строка по индексу опции; `null` — такой строки нет.
  ProtocolChoice? byIndex(int index) {
    for (final ch in choices) {
      if (ch.index == index) return ch;
    }
    return null;
  }
}

/// Инвентарь протоколов активного источника, отфильтрованный ЗАКРЕПЛЁННЫМ
/// узлом.
final protocolInventoryProvider = Provider<ProtocolInventory>((ref) {
  final options = ref.watch(protocolsProvider);
  final inventory = ref.watch(exitInventoryProvider);
  return buildProtocolInventory(
    options,
    inventory,
    onlyNodeKey: inventory.selectedNodeKey,
  );
});

/// Чистая сборка инвентаря: отделена от провайдера, чтобы проверяться без
/// поднятия ProviderContainer и всей цепочки профилей.
///
/// [onlyNodeKey] — ключ закреплённого узла. Узел найден — считаем по нему
/// одному: восемь инбаундов одной машины это её протоколы, а не протоколы
/// флота. Узел не закреплён (или ключ не нашёлся, например после смены плана)
/// — считаем по всем узлам, и [ProtocolInventory.scopedNodeKey] остаётся
/// `null`, чтобы экран назвал область честно.
ProtocolInventory buildProtocolInventory(
  List<ProtocolOption> options,
  ExitInventory inventory, {
  String? onlyNodeKey,
}) {
  final scoped = onlyNodeKey == null
      ? null
      : inventory.nodes.where((n) => n.key == onlyNodeKey).toList();
  if (scoped != null && scoped.isNotEmpty) {
    return _buildFrom(options, inventory, scoped, onlyNodeKey);
  }
  return _buildFrom(options, inventory, inventory.nodes, null);
}

ProtocolInventory _buildFrom(
  List<ProtocolOption> options,
  ExitInventory inventory,
  List<ExitNode> sourceNodes,
  String? scopedNodeKey,
) {
  // Токены узла: `vless`, но и `vless+ws+reality` из каталога CSM. Режем по
  // всему, что не буква и не цифра, — форма записи принадлежит источнику, и
  // разбирать её как грамматику значило бы завести ещё одно место, где она
  // описана.
  final perNode = <Set<String>>[];
  for (final node in sourceNodes) {
    final tokens = _tokens(node.protocol);
    if (tokens.isNotEmpty) perNode.add(tokens);
  }

  if (perNode.isEmpty) {
    final ProtocolUnavailableReason reason;
    if (inventory.source == ExitInventorySource.none) {
      reason = ProtocolUnavailableReason.noProfile;
    } else if (sourceNodes.isEmpty) {
      reason = ProtocolUnavailableReason.sourceEmpty;
    } else {
      reason = ProtocolUnavailableReason.sourceSilent;
    }
    // Молчание источника это НЕИЗВЕСТНО, а не разрешение. Раньше здесь стояло
    // `available` на каждой строке — «источник ничего не исключил, значит можно
    // всё», — и пикер обещал протоколы, которых на узле нет; выбор применялся,
    // ядро не находило нужного outbound'а и молча оставалось на прежнем.
    // Выключить список тоже нельзя: узлы при этом рабочие. Поэтому строки
    // остаются нажимаемыми, но помечены непроверенными, а причина названа.
    final unknown = reason == ProtocolUnavailableReason.noProfile
        ? ProtocolAvailability.unavailable(reason)
        : ProtocolAvailability.unknown(reason);
    return ProtocolInventory(
      source: inventory.source,
      verified: false,
      unverified: unknown,
      scopedNodeKey: scopedNodeKey,
      choices: <ProtocolChoice>[
        for (var i = 0; i < options.length; i++)
          _choiceOf(
            options[i],
            i,
            // «Авто» остаётся доступным при любом источнике: это отказ от
            // выбора, а не утверждение о флоте.
            availability: options[i].outboundTypes.isEmpty
                ? ProtocolAvailability.available
                : unknown,
          ),
      ],
    );
  }

  final known = <String>{
    for (final o in options) ...o.outboundTypes.map((t) => t.toLowerCase()),
  };
  final choices = <ProtocolChoice>[];
  final matchedNodes = List<bool>.filled(perNode.length, false);

  for (var i = 0; i < options.length; i++) {
    final o = options[i];
    if (o.outboundTypes.isEmpty) {
      // «Авто» ничему не сопоставляется: это отказ от выбора, он валиден на
      // любом флоте.
      choices.add(_choiceOf(o, i));
      continue;
    }
    final types = o.outboundTypes.map((t) => t.toLowerCase()).toSet();
    final shapes = <String>{};
    var count = 0;
    var shapeSeen = false;
    // Называет ли источник формы — вопрос ТОЛЬКО к узлам, подошедшим этой
    // опции. Считать его по всему инвентарю значило судить о vless по чужому
    // узлу: обычная сторонняя подписка с голым `vless` и голым `trojan` даёт
    // незнакомый токен `trojan`, и Reality объявлялся недоступным «формы нет»,
    // хотя про формы эта подписка не сказала ни слова — а строка при этом не
    // нажималась, и выбрать рабочий протокол было нечем.
    var matchedNamesShape = false;
    for (var n = 0; n < perNode.length; n++) {
      final tokens = perNode[n];
      if (!tokens.any(types.contains)) continue;
      matchedNodes[n] = true;
      count++;
      final extra = tokens.where((t) => !known.contains(t));
      if (extra.isNotEmpty) matchedNamesShape = true;
      shapes.addAll(extra);
      if (o.shape != null && tokens.contains(o.shape)) shapeSeen = true;
    }

    final ProtocolAvailability availability;
    if (count == 0) {
      availability = const ProtocolAvailability.unavailable(
        ProtocolUnavailableReason.notInFleet,
      );
    } else if (o.shape != null && matchedNamesShape && !shapeSeen) {
      availability = ProtocolAvailability.unavailable(
        ProtocolUnavailableReason.shapeNotInFleet,
        detail: o.shape,
      );
    } else {
      availability = ProtocolAvailability.available;
    }
    choices.add(
      _choiceOf(
        o,
        i,
        shapes: shapes.toList()..sort(),
        nodeCount: count,
        availability: availability,
      ),
    );
  }

  // Что источник раздаёт помимо списка ядра. Такой протокол нельзя выбрать —
  // но и промолчать о нём нельзя: пользователь, увидевший `naive` в описании
  // тарифа, обязан найти его в списке названным, а не искать в обновлении
  // приложения, которого не существует.
  final extras = <String, int>{};
  for (var n = 0; n < perNode.length; n++) {
    if (matchedNodes[n]) continue;
    final label = (perNode[n].toList()..sort()).join(' · ');
    extras[label] = (extras[label] ?? 0) + 1;
  }
  final extraLabels = extras.keys.toList()..sort();
  for (final label in extraLabels) {
    choices.add(
      ProtocolChoice(
        index: -1,
        name: label,
        desc: '',
        icon: Lucide.globe,
        nodeCount: extras[label]!,
        availability: const ProtocolAvailability.unavailable(
          ProtocolUnavailableReason.notRequestable,
        ),
      ),
    );
  }

  return ProtocolInventory(
    source: inventory.source,
    choices: List<ProtocolChoice>.unmodifiable(choices),
    verified: true,
    scopedNodeKey: scopedNodeKey,
  );
}

ProtocolChoice _choiceOf(
  ProtocolOption o,
  int index, {
  List<String> shapes = const <String>[],
  int nodeCount = 0,
  ProtocolAvailability availability = ProtocolAvailability.available,
}) => ProtocolChoice(
  index: index,
  name: o.name,
  desc: o.desc,
  icon: o.icon,
  recommended: o.recommended,
  auto: o.auto,
  shapes: shapes,
  nodeCount: nodeCount,
  availability: availability,
);

Set<String> _tokens(String raw) {
  final out = <String>{};
  for (final part in raw.toLowerCase().split(RegExp(r'[^a-z0-9]+'))) {
    if (part.isNotEmpty) out.add(part);
  }
  return out;
}

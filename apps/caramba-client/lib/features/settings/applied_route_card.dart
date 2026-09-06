/// Карточка «Что применилось»: применённый маршрут глазами ЯДРА.
///
/// Это ответ на «настройки по типу блок рекламы или стриминг непонятно
/// работают или нет». Раньше ответить было нечем: приложение показывало
/// выбранный пресет — то есть свой собственный запрос — и выдавало его за
/// результат. Здесь показывается только то, что ядро доложило про поднятый
/// туннель, и каждое утверждение разделено надвое: пресет ПРИМЕНЁН и его
/// правила РАЗРЕШИЛИСЬ. Второе без первого бывает, и молчать об этом нельзя —
/// именно этот разрыв пользователь и переживал как «непонятно».
///
/// Карточка не выдумывает благополучия: пока ядро не отчиталось, она говорит
/// «не отчиталось», а не рисует галочку.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/features/settings/enhancements_summary.dart';
import 'package:caramba_client/features/settings/route_report.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class AppliedRouteCard extends ConsumerWidget {
  const AppliedRouteCard({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final connected = ref.watch(vpnProvider).isConnected;
    final report = ref.watch(appliedRouteProvider).valueOrNull;
    if (report == null) return const SizedBox.shrink();

    // До первого подъёма показывать нечего: отчёт описывает СОБЫТИЕ, а не
    // настройку, и «подъёма не было» на выключенном туннеле это не скрытая
    // строка, а отсутствие события. При поднятом туннеле молчать нельзя.
    if (!report.hasReport && !connected) return const SizedBox.shrink();

    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const SectionTitle(
            'Что применилось',
            padding: EdgeInsets.only(bottom: AppSpace.s3),
          ),
          if (!report.supported)
            const InlineBanner(
              tone: BannerTone.warning,
              glyph: Lucide.alert,
              text: 'Ядро этой сборки не отчитывается о применённом маршруте. '
                  'Что именно оно применило, проверить нечем.',
            )
          else if (!report.known)
            InlineBanner(
              tone: BannerTone.warning,
              glyph: Lucide.alert,
              text: report.reason == 'not_raised'
                  ? 'Ядро ещё не отчиталось о поднятом туннеле. Пока это '
                      'состояние держится, о применённых правилах ничего не '
                      'известно.'
                  : 'Ядро не сообщило, какой маршрут применён.',
            )
          else ...[
            RowsGroup(children: _rows(context, report, connected)),
            ..._notes(report),
          ],
        ],
      ),
    );
  }

  List<Widget> _rows(BuildContext context, AppliedRoute r, bool connected) {
    final preset = r.preset;
    return <Widget>[
      CRow(
        icon: Lucide.route,
        // То же имя, что у настройки везде: лист выбора
        // (`kRouteModeSheetTitle`), строка на Главной и
        // `csmSettingTitle(CsmSettingKey.preset)` зовут её «Режим», и эта
        // строка не вправе называть ту же величину иначе.
        label: 'Режим',
        value: switch (r.source) {
          AppliedRouteSource.preset => preset == null
              ? 'пресет без имени'
              : '${preset.emoji} ${preset.name}'.trim(),
          // Ядро выбрало правила само — это не пресет, и подписывать его
          // именем последнего выбранного значило бы соврать.
          AppliedRouteSource.coreDefault => 'правила ядра по умолчанию',
          AppliedRouteSource.custom => 'правила оператора',
          AppliedRouteSource.unknown => 'ядро не назвало источник правил',
        },
      ),
      CRow(
        label: 'Правил в сборке',
        // null это «состав выбрало ядро», и это другой ответ, чем ноль.
        value: r.rules?.toString() ?? 'считает ядро',
        mono: r.rules != null,
      ),
      if (preset != null && preset.droppedRules > 0)
        CRow(
          label: 'Правил потеряно',
          value: preset.droppedRules.toString(),
          mono: true,
          valueColor: context.c.warning,
        ),
      _WrapRow(
        icon: Lucide.lock,
        label: 'Блок рекламы',
        // Источник СИЛЬНЕЕ признака пресета.
        //
        // С появлением переключателя блок рекламы приходит поверх любого
        // пресета, а `blocksAds` реестра отвечает на другой вопрос — режет ли
        // рекламу САМ пресет. Оставить здесь признак значило бы писать «этот
        // пресет его не включает» на том же экране, где переключатель говорит
        // «работает». Названный источник закрывает вопрос обоим.
        value: _adsValue(r),
      ),
      _WrapRow(
        icon: Lucide.zap,
        label: 'Стриминг через VPN',
        value: _capabilityValue(r.routesStreaming, r),
      ),
      CRow(
        icon: Lucide.waypoints,
        label: 'Вход (relay)',
        value: switch (r.relay.state) {
          AppliedRelayState.notRequested => 'не запрашивался',
          AppliedRelayState.ignored => 'отброшен',
          AppliedRelayState.sent => r.relay.chained
              ? 'цепочка в конфиге есть'
              : 'цепочки в конфиге нет',
          AppliedRelayState.unknown => 'неизвестно',
        },
      ),
      if (!connected)
        const CRow(
          label: 'Туннель',
          value: 'снят; показан последний применённый',
        ),
    ];
  }

  /// Значение строки блока рекламы: сперва по названному источнику списка,
  /// и только если его нет — по признаку пресета.
  String _adsValue(AppliedRoute r) {
    final ads = adsSourceOf(r);
    if (ads == null) return _capabilityValue(r.blocksAds, r);
    return switch (ads.state) {
      RuleSourceState.file => 'включён, правил ${ads.keptRules}',
      RuleSourceState.mirror => 'включён, список с зеркала',
      RuleSourceState.dropped => 'включён, но список не доехал',
      RuleSourceState.unknown => 'включён, состояние списка неизвестно',
    };
  }

  /// Значение строки возможности. Три исхода, а не два: пресет её включает,
  /// пресет её не включает, и — отдельно — сборка про неё вообще не говорит.
  String _capabilityValue(bool? on, AppliedRoute r) {
    if (on == null) {
      return r.source == AppliedRouteSource.preset
          ? 'пресет ядру этой сборки неизвестен'
          : 'пресет не применялся';
    }
    if (!on) return 'этот пресет его не включает';
    // Включён — но правда ли он работает, решает база GEOSITE, и ответ на это
    // стоит отдельной строкой ниже. Здесь только факт применения.
    return switch (r.geosite.resolved) {
      true => 'включён, списки на месте',
      false => 'включён, но правила мертвы',
      null => 'включён, подтвердить нечем',
    };
  }

  /// Причины под строками: их место здесь, а не в значении строки, — причина
  /// не помещается в правую колонку и там всегда обрезалась бы.
  List<Widget> _notes(AppliedRoute r) {
    final out = <Widget>[];

    void add(BannerTone tone, String glyph, String text) {
      out
        ..add(const SizedBox(height: AppSpace.s3))
        ..add(InlineBanner(tone: tone, glyph: glyph, text: text));
    }

    // База GEOSITE упоминается только когда пресет от неё зависит: на
    // `global` и `ru-full` это лишний абзац про чужую проблему.
    if (r.geosite.required) {
      add(
        switch (r.geosite.resolved) {
          true => BannerTone.info,
          false => BannerTone.danger,
          null => BannerTone.warning,
        },
        Lucide.shield,
        r.geosite.message,
      );
    }

    // Списки перечисляются ВСЕ, а не только выброшенные. «Пресет применён» и
    // «его правила доехали» — разные утверждения, и вопрос владельца был
    // именно про второе: увидеть, что источники правил разрешились, можно
    // только если их назвали поимённо. Показывать одни провалы значило бы
    // оставить успех неотличимым от отсутствия проверки.
    for (final s in r.preset?.sources ?? const <AppliedRuleSource>[]) {
      add(
        s.isDropped ? BannerTone.warning : BannerTone.info,
        Lucide.fileCheck,
        'Список правил «${s.name}»: ${s.message}.',
      );
    }

    if (r.relay.state == AppliedRelayState.sent && !r.relay.chained) {
      add(BannerTone.warning, Lucide.waypoints, r.relay.message);
    } else if (r.relay.state == AppliedRelayState.ignored) {
      add(BannerTone.warning, Lucide.waypoints, r.relay.message);
    }

    return out;
  }
}

/// Строка карточки со свободной высотой: значение переносится на вторую
/// строку под подписью вместо того, чтобы обрезаться многоточием.
///
/// [CRow] кладёт подпись и значение в одну строку и режет значение
/// [TextOverflow.ellipsis] — но обрезка у dart:ui включается уже тем, что
/// `overflow` задан, ДАЖЕ без `maxLines`: движок молча подставляет
/// `maxLines: 1`, когда есть `ellipsis` и явного предела нет. На узком экране
/// это резало «этот пресет его не включает» до «этот пресет его не …» —
/// ровно на границе слова, где теряется отрицание. [CRow] общий для всего
/// приложения и здесь не трогается: у этих двух строк ответ пресета может
/// быть длинным сам по себе, и им нужна не более широкая колонка (её и так
/// делят пополам с подписью), а разрешение перенестись.
class _WrapRow extends StatelessWidget {
  final String icon;
  final String label;
  final String value;

  const _WrapRow({
    required this.icon,
    required this.label,
    required this.value,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      constraints: const BoxConstraints(minHeight: 54),
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpace.s4,
        vertical: AppSpace.s3,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          LucideIcon(icon, color: c.textMed, size: 20),
          const SizedBox(width: AppSpace.s3 + 2),
          Expanded(
            child: Text(label, style: AppType.bodyMd.copyWith(color: c.textHi)),
          ),
          const SizedBox(width: AppSpace.s3),
          Flexible(
            child: Text(
              value,
              textAlign: TextAlign.right,
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
          ),
        ],
      ),
    );
  }
}

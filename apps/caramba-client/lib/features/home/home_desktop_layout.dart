/// Двухпанельная раскладка «Подключения» на десктопе (DESKTOP-SPEC.md, 5.1).
///
/// Здесь НЕТ ни одного `ref` и ни одного решения о данных: всё, что раскладка
/// рисует, приходит параметрами из [HomeScreen]. Причина простая — ветка
/// выбирается один раз, уже после того как экран посчитал стадию, доступ,
/// карточки и пустое состояние; сочини раскладка что-нибудь своё, и десктоп
/// начал бы расходиться с мобильным на тех же данных.
///
/// Отличие от мобильной колонки ровно в композиции. Слева неподвижная панель
/// 420: шапка, дайл, адрес прокси, автоподбор и то, что относится к САМОМУ
/// подключению (баннер реконнекта, карточка закрытого доступа). Справа
/// прокручиваемый список карточек. Левая панель не скроллится намеренно:
/// атмосферный слой измеряется по ключам дайла и шапки, и `shift` в
/// `_measureAnchor` равен нулю только пока эта панель стоит на месте.
library;

import 'package:flutter/material.dart';

import 'package:caramba_client/atmosphere/atmosphere_layer.dart';
import 'package:caramba_client/data/models/subscription.dart' show AccessState;
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/features/home/home_screen.dart'
    show CardsBackdrop;
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class HomeDesktopLayout extends StatelessWidget {
  /// Ключ атмосферного слоя: по нему [HomeScreen] считает геометрию дайла.
  final GlobalKey layerKey;

  /// Ключ строки-шапки левой панели. Высота шапки задаёт верхнюю границу
  /// атмосферы, поэтому строка есть даже когда в ней нет ни одного контрола.
  final GlobalKey headerKey;

  final VpnStage stage;
  final AtmosphereAnchor anchor;

  /// План и колокол на панельном пути, пустая коробка высотой 44 на гостевом.
  final Widget headerTrailing;

  /// Готовый `ConnectDial` вместе с его тикером и ключами: те же параметры и
  /// тот же `onTap`, что на мобильном пути.
  final Widget dial;

  /// Адрес локального инбаунда в proxy-режиме. `null` — ядро в TUN.
  final String? proxyEndpoint;

  /// Кнопка автоподбора. `null` — подключений нет вовсе, подбирать не из чего.
  final Widget? autopilot;

  final bool needsReconnect;

  /// Отказ в доступе. Карточку рисуем только когда доступ действительно закрыт.
  final AccessState? access;

  /// Карточки правой колонки в том же порядке, что и на мобильном экране.
  final List<Widget> cards;

  /// Подключений нет вовсе: правая колонка показывает пустое состояние.
  final bool noConnections;

  final VoidCallback onAddConnection;
  final VoidCallback onConnectPanel;

  const HomeDesktopLayout({
    required this.layerKey,
    required this.headerKey,
    required this.stage,
    required this.anchor,
    required this.headerTrailing,
    required this.dial,
    required this.cards,
    required this.noConnections,
    required this.onAddConnection,
    required this.onConnectPanel,
    this.proxyEndpoint,
    this.autopilot,
    this.needsReconnect = false,
    this.access,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Scaffold(
      backgroundColor: c.bgBase,
      body: Stack(
        children: [
          // Атмосфера full-bleed под обеими колонками: она декоративна и
          // исключена из семантики, как и на мобильном.
          Positioned.fill(
            child: AtmosphereLayer(key: layerKey, stage: stage, anchor: anchor),
          ),
          Positioned.fill(
            child: Padding(
              padding: const EdgeInsets.all(DesktopTokens.contentPad),
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(
                    maxWidth: DesktopTokens.contentMaxWidth,
                  ),
                  child: Row(
                    // stretch, а не center: правая колонка это ListView, и без
                    // ограниченной высоты она не соберётся вовсе.
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      SizedBox(
                        width: DesktopTokens.homeLeftPane,
                        child: _leftPane(context),
                      ),
                      const SizedBox(width: DesktopTokens.columnGap),
                      Expanded(child: _rightPane(context)),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _leftPane(BuildContext context) {
    final c = context.c;
    final blocked = access != null && access!.isBlocked;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Высота шапки здесь такая же, как на мобильном (44): порядок
        // «зажигания» маршрутов атмосферы зарегистрирован на эту геометрию.
        SizedBox(
          height: 44,
          child: Row(
            key: headerKey,
            children: [const Spacer(), headerTrailing],
          ),
        ),
        const SizedBox(height: AppSpace.s8),
        Center(child: dial),
        if (proxyEndpoint != null) ...[
          const SizedBox(height: AppSpace.s2),
          Text(
            'Прокси $proxyEndpoint',
            textAlign: TextAlign.center,
            style: AppType.monoSm.copyWith(color: c.textLow),
          ),
        ],
        const SizedBox(height: AppSpace.s6),
        if (autopilot != null) autopilot!,
        // Баннер реконнекта и карточка закрытого доступа говорят про САМО
        // подключение, а не про карточки выбора, поэтому на десктопе они
        // стоят под дайлом, а не в правом списке.
        if (needsReconnect) ...[
          const SizedBox(height: AppSpace.s4),
          const ReconnectBanner(),
        ],
        if (blocked) ...[
          const SizedBox(height: AppSpace.s4),
          AccessCard(access: access),
        ],
      ],
    );
  }

  Widget _rightPane(BuildContext context) {
    if (noConnections) return _empty(context);
    return ListView(
      padding: const EdgeInsets.only(bottom: AppSpace.s8),
      children: [CardsBackdrop(children: cards)],
    );
  }

  /// Пустое состояние десктопа. Отличие от мобильного одно, и оно
  /// принципиальное: кнопки НЕ на всю ширину. Растянутая на 700 px кнопка
  /// «Добавить подключение» — главный признак мобильного экрана, открытого в
  /// окне, и E1 показала ровно её.
  Widget _empty(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.only(bottom: AppSpace.s8),
      children: [
        Align(
          alignment: Alignment.topLeft,
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: DesktopTokens.dialogMaxWidth,
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const ScreenEmpty(
                  glyph: Lucide.plus,
                  title: 'Подключений пока нет',
                  message: 'Добавьте ссылку на подписку, конфиг или ссылку '
                      'caramba:// из бота оператора — приложение само разберёт, '
                      'что это. Настройки и профиль можно посмотреть уже '
                      'сейчас.',
                ),
                Wrap(
                  spacing: AppSpace.s3,
                  runSpacing: AppSpace.s3,
                  children: [
                    FilledButton(
                      onPressed: onAddConnection,
                      style: FilledButton.styleFrom(
                        minimumSize: const Size(200, 44),
                      ),
                      child: const Text('Добавить подключение'),
                    ),
                    OutlinedButton.icon(
                      onPressed: onConnectPanel,
                      style: OutlinedButton.styleFrom(
                        minimumSize: const Size(200, 44),
                      ),
                      icon: LucideIcon(
                        Lucide.appWindow,
                        color: context.c.textHi,
                        size: 18,
                      ),
                      label: const Text('Подключить панель'),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

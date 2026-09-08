/// Баннер переподключения: одно место, где приложение отчитывается о судьбе
/// изменённых настроек и пути.
///
/// Половина «путь» (страна/узел, relay, тип подключения, режим) применяется
/// САМА, через окно тишины (`auto_reconnect.dart`), и баннер тогда говорит, что
/// произойдёт, что происходит и чем кончилось. Половина «настройки» (реклама,
/// сайты, DNS, стек, MTU, IPv6, FakeIP, kill switch, режим захвата) по-прежнему
/// ждёт человека: их правят сериями, часть из них диагностическая, и момент
/// разрыва там выбирает он.
///
/// Успех показан СТРОКОЙ В ЭТОМ ЖЕ БАННЕРЕ, а не тостом. Тосту нужен
/// `BuildContext` ровно в тот миг, когда ядро доложило `Up`, а машина
/// переподключения контекста не имеет и иметь не должна; баннер же в этот миг
/// заведомо на экране. Заодно новость появляется там же, где стояло обещание,
/// — читается как продолжение одной фразы, а не как второе сообщение.
library;

import 'dart:async';

import 'package:flutter/material.dart';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/state/auto_reconnect.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

class ReconnectBanner extends ConsumerWidget {
  const ReconnectBanner({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final auto = ref.watch(autoReconnectProvider);
    final busy = auto.phase == AutoReconnectPhase.reconnecting;

    final message = switch (auto.phase) {
      AutoReconnectPhase.idle => kManualReconnectText,
      _ => auto.message.isEmpty ? kManualReconnectText : auto.message,
    };

    return Container(
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Row(
        children: [
          if (busy)
            SizedBox(
              width: 18,
              height: 18,
              child: CircularProgressIndicator(strokeWidth: 2, color: c.textHi),
            )
          else
            LucideIcon(
              _glyph(auto.phase),
              color: auto.phase == AutoReconnectPhase.done
                  ? c.accent
                  : c.warning,
              size: 18,
            ),
          const SizedBox(width: AppSpace.s3),
          Expanded(
            child: Text(
              message,
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          ),
          ..._action(context, ref, auto),
        ],
      ),
    );
  }

  /// Кнопка есть не всегда: пока попытка в полёте и пока держится строка
  /// успеха, нажимать нечего — предлагать действие в этот момент значило бы
  /// звать человека рвать то, что и так поднимается.
  List<Widget> _action(
    BuildContext context,
    WidgetRef ref,
    AutoReconnectState auto,
  ) {
    final (String, VoidCallback)? spec = switch (auto.phase) {
      // Отказ от автоматики. Выбор при этом НЕ откатывается: приложение не
      // имеет права молча менять то, что человек выбрал.
      AutoReconnectPhase.pending => (
        'Не сейчас',
        () => ref.read(autoReconnectProvider.notifier).dismiss(),
      ),
      AutoReconnectPhase.failed => ('Повторить', () => _reconnect(ref)),
      AutoReconnectPhase.reconnecting || AutoReconnectPhase.done => null,
      AutoReconnectPhase.idle ||
      AutoReconnectPhase.manual => ('Переподключить', () => _reconnect(ref)),
    };
    if (spec == null) return const <Widget>[];

    final c = context.c;
    return <Widget>[
      const SizedBox(width: AppSpace.s3),
      // Кнопка по содержимому, а НЕ во всю ширину: в [Row] рядом с
      // [Expanded] нефлексовый ребёнок получает бесконечную ширину, и
      // `width: double.infinity` внутри него роняет разметку. Баннер
      // обязан пережить собственное появление.
      TextButton(
        style: TextButton.styleFrom(
          foregroundColor: c.textHi,
          minimumSize: const Size(0, 40),
          padding: const EdgeInsets.symmetric(horizontal: AppSpace.s3),
        ),
        onPressed: spec.$2,
        child: Text(spec.$1),
      ),
    ];
  }

  /// Ручное переподключение идёт ЧЕРЕЗ машину: она одна умеет дождаться
  /// результата и вернуть туннель, если он не поднялся. Прямой
  /// `disconnect()+connect()` из кнопки оставлял бы человека в отказе — с kill
  /// switch это «вообще без сети».
  static void _reconnect(WidgetRef ref) {
    unawaited(ref.read(autoReconnectProvider.notifier).reconnectNow());
  }

  static String _glyph(AutoReconnectPhase phase) => switch (phase) {
    AutoReconnectPhase.done => Lucide.check,
    _ => Lucide.alert,
  };
}

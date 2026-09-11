import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/ticket.dart';
import 'package:caramba_client/features/profile/panel_required.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/tickets_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Новый запрос в поддержку: категория + тема + сообщение. После создания
/// открывает тикет. Категория уходит на панель как есть: по ней админка и бот
/// фильтруют и маршрутизируют тикеты, а без неё всё падало в `general`.
class NewTicketScreen extends ConsumerStatefulWidget {
  const NewTicketScreen({super.key});

  @override
  ConsumerState<NewTicketScreen> createState() => _NewTicketScreenState();
}

class _NewTicketScreenState extends ConsumerState<NewTicketScreen> {
  final _subject = TextEditingController();
  final _message = TextEditingController();
  TicketCategory _category = TicketCategory.general;
  bool _sending = false;

  @override
  void dispose() {
    _subject.dispose();
    _message.dispose();
    super.dispose();
  }

  bool get _valid =>
      _subject.text.trim().isNotEmpty && _message.text.trim().isNotEmpty;

  Future<void> _submit() async {
    if (!_valid || _sending) return;
    setState(() => _sending = true);
    try {
      final id = await ref
          .read(apiClientProvider)
          .createTicket(
            subject: _subject.text.trim(),
            message: _message.text.trim(),
            category: _category.value,
          );
      ref.invalidate(ticketsProvider);
      if (!mounted) return;
      if (id > 0) {
        context.go('${AppRoute.tickets}/$id');
      } else {
        context.go(AppRoute.tickets);
      }
      showCarambaToast(context, 'Запрос отправлен');
    } on ApiException catch (e) {
      if (mounted) showCarambaToast(context, e.message);
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    // Раздел живёт у аккаунта панели: в generic-режиме показываем, что нужно
    // сделать, чтобы он заработал, вместо 401 за каждым провайдером.
    if (ref.watch(authProvider).stage != AuthStage.authenticated) {
      return const PanelRequiredScreen(title: 'Новый запрос');
    }
    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s12,
          ),
          children: [
            ScreenHead(
              'Новый запрос',
              trailing: IconBtn(Lucide.x, onTap: () => _back(context)),
            ),
            Text(
              'Опишите проблему как можно конкретнее: что делали, что ожидали, что произошло.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s5),
            const SectionTitle(
              'Категория',
              padding: EdgeInsets.only(bottom: AppSpace.s2),
            ),
            _categoryField(context),
            const SizedBox(height: AppSpace.s5),
            const SectionTitle(
              'Тема',
              padding: EdgeInsets.only(bottom: AppSpace.s2),
            ),
            _field(
              context,
              controller: _subject,
              hint: 'Кратко: о чём запрос',
              maxLines: 1,
              onChanged: () => setState(() {}),
            ),
            const SizedBox(height: AppSpace.s5),
            const SectionTitle(
              'Сообщение',
              padding: EdgeInsets.only(bottom: AppSpace.s2),
            ),
            _field(
              context,
              controller: _message,
              hint: 'Подробное описание',
              minLines: 5,
              maxLines: 12,
              onChanged: () => setState(() {}),
            ),
            const SizedBox(height: AppSpace.s6),
            FilledButton(
              onPressed: (_valid && !_sending) ? _submit : null,
              child: _sending
                  ? SizedBox(
                      width: 20,
                      height: 20,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: c.textOnAccent,
                      ),
                    )
                  : const Text('Отправить'),
            ),
          ],
        ),
      ),
    );
  }

  /// Выпадающий список категорий в той же рамке, что и текстовые поля.
  /// Подписи русские, как и весь остальной интерфейс приложения (английские
  /// лежат в `TicketCategory.labelEn` для мини-аппа и будущей локализации);
  /// значения — серверные (`ALLOWED_TICKET_CATEGORIES`).
  Widget _categoryField(BuildContext context) {
    final c = context.c;
    return DropdownButtonFormField<TicketCategory>(
      key: const Key('ticket_category'),
      initialValue: _category,
      isExpanded: true,
      dropdownColor: c.surface2,
      iconEnabledColor: c.textMed,
      style: AppType.bodyMd.copyWith(color: c.textHi),
      decoration: _decoration(context, hint: null),
      items: [
        for (final cat in TicketCategory.values)
          DropdownMenuItem<TicketCategory>(
            value: cat,
            child: Text(
              cat.label,
              style: AppType.bodyMd.copyWith(color: c.textHi),
            ),
          ),
      ],
      onChanged: _sending
          ? null
          : (v) => setState(() => _category = v ?? TicketCategory.general),
    );
  }

  Widget _field(
    BuildContext context, {
    required TextEditingController controller,
    required String hint,
    int? minLines,
    int maxLines = 1,
    required VoidCallback onChanged,
  }) {
    final c = context.c;
    return TextField(
      controller: controller,
      minLines: minLines,
      maxLines: maxLines,
      onChanged: (_) => onChanged(),
      style: AppType.bodyMd.copyWith(color: c.textHi),
      decoration: _decoration(context, hint: hint),
    );
  }

  /// Общая рамка полей формы: одна для текста и для выпадающего списка,
  /// чтобы категория не выглядела чужеродно.
  InputDecoration _decoration(BuildContext context, {required String? hint}) {
    final c = context.c;
    return InputDecoration(
      hintText: hint,
      hintStyle: AppType.bodyMd.copyWith(color: c.textLow),
      filled: true,
      fillColor: c.surface1,
      contentPadding: const EdgeInsets.symmetric(
        horizontal: AppSpace.s4,
        vertical: AppSpace.s3,
      ),
      border: OutlineInputBorder(
        borderRadius: AppRadius.r14,
        borderSide: BorderSide(color: c.borderSubtle),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: AppRadius.r14,
        borderSide: BorderSide(color: c.borderSubtle),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: AppRadius.r14,
        borderSide: BorderSide(color: c.borderStrong),
      ),
    );
  }

  void _back(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.tickets);
    }
  }
}

/// «Что это приложение отправляет», INV-20.
///
/// Перечисление КАЖДОГО поля, которое клиент передаёт оператору, с кнопкой
/// копирования. Экран одновременно три вещи: доказательство по Apple 2.3.1,
/// что ничего не спрятано, артефакт раскрытия по 5.1.1(i) и единственный
/// реалистичный инструмент поддержки. Он дёшев только потому, что запрос
/// маленький, и это само по себе довод держать его маленьким.
///
/// Вторая половина экрана не менее важна первой: список того, что не уходит
/// НИКОГДА (02-SPEC.md 7.10, INV-15). Утверждение «мы не отправляем список
/// ваших приложений» проверяемо только рядом с полным списком того, что мы
/// отправляем.
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_write.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Одно поле запроса: имя на проводе, что это, и что лежит там сейчас.
typedef SentField = ({String wire, String meaning, String value});

/// Один запрос: метод, путь, зачем и какие поля несёт.
typedef SentRequest = ({
  String method,
  String path,
  String purpose,
  List<SentField> fields,
});

class WhatWeSendScreen extends ConsumerWidget {
  const WhatWeSendScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final ready = ref.watch(connectionProfilesReadyProvider);
    final csm = ref.watch(csmProfileStateProvider);
    final requests = buildSentRequests(csm);

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
              'Что мы отправляем',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            Text(
              'Полный список полей, которые это приложение передаёт оператору. '
              'Ничего сверх этого списка не уходит.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s4),
            GhostButton(
              label: 'Скопировать весь список',
              icon: Lucide.copy,
              onPressed: () async {
                await Clipboard.setData(
                  ClipboardData(text: sentAsText(requests, csm)),
                );
                if (context.mounted) {
                  showCarambaToast(context, 'Список скопирован');
                }
              },
            ),
            const SizedBox(height: AppSpace.s2),
            if (!ready)
              const Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SectionTitle('Запросы'),
                  SkeletonRows(rows: 4),
                  SizedBox(height: AppSpace.s4),
                  SkeletonRows(rows: 4),
                ],
              )
            else ...[
              if (csm == null) ...[
                const SizedBox(height: AppSpace.s2),
                const InlineBanner(
                  tone: BannerTone.info,
                  glyph: Lucide.alert,
                  text:
                      'Профиль не проходил энроллмент CSM. Значения полей ниже '
                      'пустые, но состав запросов от этого не меняется: он '
                      'вкомпилирован, а не получен от оператора.',
                ),
              ],
              for (final r in requests) ...[
                SectionTitle('${r.method} ${r.path}'),
                Padding(
                  padding: const EdgeInsets.only(bottom: AppSpace.s3),
                  child: Text(
                    r.purpose,
                    style: AppType.bodySm.copyWith(color: c.textMed),
                  ),
                ),
                RowsGroup(
                  children: [
                    for (final f in r.fields)
                      CRow(
                        label: f.wire,
                        mono: true,
                        value: f.value,
                        trailing: null,
                      ),
                  ],
                ),
                const SizedBox(height: AppSpace.s2),
                for (final f in r.fields)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 2),
                    child: Text(
                      '${f.wire}: ${f.meaning}',
                      style: AppType.bodySm.copyWith(color: c.textLow),
                    ),
                  ),
              ],

              const SectionTitle('Что не уходит никогда'),
              RowsGroup(
                children: [
                  for (final line in kCsmNeverSent)
                    CRow(icon: Lucide.x, label: line),
                ],
              ),
              const SizedBox(height: AppSpace.s3),
              Text(
                'Список установленных приложений не имеет ключа в реестре '
                'настроек, поэтому сериализатор физически не способен его '
                'написать: запрет не зависит от дисциплины вызова.',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),

              const SectionTitle('Подпись запроса'),
              const RowsGroup(
                children: [
                  CRow(
                    label: 'X-CSM-Proof',
                    mono: true,
                    value: 'ECDSA P-256, r||s',
                  ),
                  CRow(label: 'предобраз', mono: true, value: 'csm1-write'),
                ],
              ),
              const SizedBox(height: AppSpace.s2),
              Text(
                'Записи настроек подписываются ключом устройства над строкой '
                'sha256("csm1-write" || 0 || метод || 0 || канонический путь '
                '|| 0 || sha256(тела)). Подписывается тело целиком, а не '
                'только заголовки: заголовок в пути переписывается, тело нет.',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ],
          ],
        ),
      ),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.settings);
    }
  }
}

/// Что не пересекает границу никогда (02-SPEC.md 7.10, INV-15).
const List<String> kCsmNeverSent = <String>[
  'Список приложений раздельного туннелирования, в обе стороны',
  'Какая ступень транспорта принесла запрос',
  'Какие ступени вы включили или выключили',
  'История попыток с этого экрана транспорта',
  'Любой текст оператора обратно ему же',
  'Любое состояние клиента сверх номера версии директивы',
];

/// Состав запросов и значения полей на сейчас.
///
/// Состав вкомпилирован и не зависит от того, что прислал оператор: подписанный
/// документ не вправе добавить приложению поле, которое оно отправит.
List<SentRequest> buildSentRequests(CsmProfileState? csm) {
  final loc = csm?.locator ?? 'нет';
  final dtp = csm?.deviceThumbprint ?? 'нет';
  final directiveVer = csm?.directive?.version ?? 0;
  final keyVer = csm?.keyDocument?.version ?? 0;
  final catId = csm?.catalog?.frameDigest ?? 'нет';

  final queued = csm?.writeQueue.entries ?? const <CsmQueuedWrite>[];

  return <SentRequest>[
    (
      method: 'GET',
      path: '/sub/k1',
      purpose:
          'Ключевой документ оператора. Без параметров он анонимен; с since '
          'приложение просит промежуточные версии, чтобы догнать ротацию '
          'корня без пропуска версий.',
      fields: <SentField>[
        (
          wire: 'since',
          meaning:
              'номер версии ключевого документа, которому вы уже доверяете',
          value: '$keyVer',
        ),
      ],
    ),
    (
      method: 'GET',
      path: '/sub/m1/{loc}',
      purpose:
          'Директива этого устройства. Единственный регулярный запрос: '
          'приложение ходит за ней по расписанию из подписанного ttl.',
      fields: <SentField>[
        (
          wire: 'loc',
          meaning: 'локатор подписки, 24 символа, выдан при энроллменте',
          value: loc,
        ),
        (
          wire: 'n',
          meaning:
              'одноразовое число, 16 свежих случайных байт на каждый запрос; '
              'оно возвращается внутри подписанного ответа',
          value: 'свежее на каждый запрос',
        ),
        (
          wire: 'v',
          meaning:
              'наибольший номер версии директивы, который это устройство '
              'приняло; это ВЕСЬ отчёт клиента о себе',
          value: '$directiveVer',
        ),
        (
          wire: 'd',
          meaning:
              'отпечаток ключа этого устройства: он говорит, на чей ключ '
              'запечатать ответ',
          value: dtp,
        ),
      ],
    ),
    (
      method: 'GET',
      path: '/sub/c1/{cat_id}/{i}',
      purpose:
          'Части каталога узлов. Адресуются содержимым, поэтому одинаковы для '
          'всех подписчиков одного тарифа.',
      fields: <SentField>[
        (
          wire: 'cat_id',
          meaning: 'хеш каталога, названный директивой',
          value: catId,
        ),
        (wire: 'i', meaning: 'номер части, с нуля', value: 'по числу частей'),
        (
          wire: 'X-CSM-Loc',
          meaning: 'локатор в заголовке: без него часть каталога не выдаётся',
          value: loc,
        ),
      ],
    ),
    (
      method: 'GET',
      path: '/sub/r1/{loc}',
      purpose: 'Резервный пул адресов на случай, когда основной путь закрыт.',
      fields: <SentField>[
        (wire: 'loc', meaning: 'локатор подписки', value: loc),
      ],
    ),
    (
      method: 'PUT',
      path: kCsmWritePathPreferences,
      purpose:
          'Запись настроек. Уходит только когда вы что-то поменяли, и только '
          'те ключи, которые вы поменяли.',
      fields: <SentField>[
        (wire: 'v', meaning: 'версия формата запроса, всегда 1', value: '1'),
        (
          wire: 'nonce',
          meaning: '16 свежих случайных байт',
          value: 'свежее на каждый запрос',
        ),
        (wire: 'dtp', meaning: 'отпечаток ключа этого устройства', value: dtp),
        (
          wire: 'want',
          meaning:
              'карта настроек из закрытого реестра: только ключи, которые вы '
              'изменили, и только значения из закрытых словарей',
          value: queued.isEmpty
              ? 'сейчас пусто'
              : queued.map((e) => csmSettingTitle(e.key)).join(', '),
        ),
        (
          wire: 'If-Match',
          meaning: 'номер версии директивы, которую правит эта запись',
          value: '$directiveVer',
        ),
      ],
    ),
    (
      method: 'POST',
      path: kCsmWritePathEnrollCode,
      purpose:
          'Энроллмент по коду. Происходит один раз, при подключении '
          'оператора.',
      fields: <SentField>[
        (
          wire: 'code',
          meaning: 'код энроллмента, который вам продиктовали',
          value: 'один раз, при подключении',
        ),
        (
          wire: 'spki',
          meaning: 'открытая половина ключа, созданного на этом устройстве',
          value: 'открытый ключ устройства',
        ),
        (wire: 'dtp', meaning: 'отпечаток этого открытого ключа', value: dtp),
        (
          wire: 'nonce',
          meaning: '16 свежих случайных байт',
          value: 'свежее на каждый запрос',
        ),
      ],
    ),
  ];
}

/// Тот же список плоским текстом для буфера обмена.
String sentAsText(List<SentRequest> requests, CsmProfileState? csm) {
  final sb = StringBuffer()
    ..writeln('Caramba Connect: что приложение отправляет оператору')
    ..writeln(
      'Составлено ${csmDateTime(DateTime.now().millisecondsSinceEpoch)}',
    )
    ..writeln();
  if (csm != null) {
    sb
      ..writeln('Оператор: ${csm.pin.pid}')
      ..writeln('Отпечаток корневого ключа: ${csm.pin.fingerprint}')
      ..writeln();
  }
  for (final r in requests) {
    sb
      ..writeln('${r.method} ${r.path}')
      ..writeln('  ${r.purpose}');
    for (final f in r.fields) {
      sb.writeln('  ${f.wire} = ${f.value}');
      sb.writeln('    ${f.meaning}');
    }
    sb.writeln();
  }
  sb.writeln('Не уходит никогда:');
  for (final line in kCsmNeverSent) {
    sb.writeln('  - $line');
  }
  return sb.toString();
}

/// Что человек вставил в единственное поле экрана подключения.
///
/// ЗАЧЕМ ЭТОТ ФАЙЛ. Экран входа спрашивал у человека то, чего он не знает:
/// имя профиля, формат конфига (пять вариантов), а до всего этого — какой из
/// трёх разделов ему нужен, «Подписка», «Панель Caramba» или «Код из бота».
/// Разделить строку по этим корзинам приложение умеет само, и умело всегда:
/// ровно та же развилка уже стоит в [DeepLinkHandler.targetOf] и срабатывает,
/// когда та же самая строка приходит не из буфера обмена, а операционной
/// системой. Единственная разница между двумя путями была в том, что по одному
/// приложение решало само, а по другому спрашивало.
///
/// ПОЧЕМУ ОТДЕЛЬНОЙ ЧИСТОЙ ФУНКЦИЕЙ, А НЕ В ЭКРАНЕ. Разбор ссылки — это то,
/// что обязано быть одинаковым в четырёх местах: поле ввода, QR, вставка из
/// буфера и файл. Копия логики в каждом из них означала бы, что `caramba://`
/// из QR работает, а он же из файла — нет, и заметить это можно только руками.
/// Чистая функция ещё и проверяема без виджетов: тест на согласие с
/// [DeepLinkHandler.targetOf] стоит рядом и ловит расхождение двух входов.
///
/// ЧТО ЗДЕСЬ НЕ РЕШАЕТСЯ. Формат конфига (clash / sing-box / v2ray / uri).
/// Его определяет ядро (`subimport.detectFormat`), у которого есть сами байты,
/// и определяет лучше: приложение видит ту же строку, но без парсеров. Здесь
/// решается только КУДА строку нести — на подтверждение приглашения, в
/// энроллмент, качать по ссылке или сразу отдать ядру.
///
/// ОТКАЗ ОБЯЗАН ОСТАТЬСЯ ОБЪЯСНИМЫМ. Ссылка нашей схемы, которую мы отвергли
/// (http:// вместо https, пустой код), возвращается как [EntryKind.refused] с
/// готовой фразой из [LinkRefusal.message], а не сваливается в «конфиг» — иначе
/// человек увидел бы ошибку разбора YAML там, где проблема в одной букве «s».
library;

import 'package:caramba_client/data/models/csm_enrollment.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/features/enroll/connect_link.dart';
import 'package:caramba_client/router/routes.dart';

/// Чем оказалась вставленная строка.
enum EntryKind {
  /// Пусто (или одни пробелы).
  empty,

  /// `caramba://connect?d=...` — самоописывающееся приглашение панели.
  /// Ведёт на экран подтверждения, где видно оператора и адрес.
  connectLink,

  /// `carambaconnect://enroll?panel=...&code=...` (в т.ч. с пином CSM).
  enrollLink,

  /// Ссылка на подписку: голый `http(s)://` или `carambaconnect://import`.
  /// Тело качает приложение, ядро получает уже текст.
  subscriptionUrl,

  /// Сам конфиг: YAML, JSON, base64-список или одиночный `vless://`-URI.
  /// Разбирается ядром с автоопределением формата.
  configText,

  /// Ссылка НАШЕЙ схемы, которую мы отвергли, и причина известна.
  refused,
}

/// Результат классификации. Ровно одно поле несёт полезную нагрузку, какое —
/// определяет [kind].
class EntryClassification {
  final EntryKind kind;

  /// Локация роутера для [EntryKind.connectLink] и [EntryKind.enrollLink].
  /// Строится теми же параметрами, что и у deeplink-хендлера.
  final String? target;

  /// URL подписки для [EntryKind.subscriptionUrl]. Для `carambaconnect://
  /// import?url=...` это ВНУТРЕННИЙ адрес, а не сама ссылка-обёртка.
  final String? url;

  /// Готовая фраза для [EntryKind.refused].
  final String? refusalMessage;

  const EntryClassification._(
    this.kind, {
    this.target,
    this.url,
    this.refusalMessage,
  });

  /// Строку можно нести дальше без сети: она уже сама себе адрес.
  bool get isLink =>
      kind == EntryKind.connectLink || kind == EntryKind.enrollLink;
}

/// Классифицирует вставленную строку.
///
/// Порядок проверок нормативен и повторяет [DeepLinkHandler.targetOf]:
/// приглашение панели идёт первым (своя схема, ни один парсер ниже его не
/// узнает), разбор CSM — раньше старого энроллмента (иначе теряется параметр
/// `k`, то есть пин ссылки, и закреплённый энроллмент молча становится
/// незакреплённым).
EntryClassification classifyEntry(String raw) {
  final trimmed = raw.trim();
  if (trimmed.isEmpty) return const EntryClassification._(EntryKind.empty);

  if (looksLikeConnectLink(trimmed)) {
    return EntryClassification._(
      EntryKind.connectLink,
      target: Uri(
        path: AppRoute.connect,
        queryParameters: {'link': trimmed},
      ).toString(),
    );
  }

  final csm = CsmEnrollLink.tryParse(trimmed);
  if (csm != null) {
    final pin = csm.linkPin;
    return EntryClassification._(
      EntryKind.enrollLink,
      target: Uri(
        path: AppRoute.enroll,
        queryParameters: {
          'panel': csm.origin,
          'code': csm.code,
          if (pin != null) 'k': pin,
        },
      ).toString(),
    );
  }

  final enroll = EnrollLink.tryParse(trimmed);
  if (enroll != null) {
    return EntryClassification._(
      EntryKind.enrollLink,
      target: Uri(
        path: AppRoute.enroll,
        queryParameters: {'panel': enroll.panelUrl, 'code': enroll.code},
      ).toString(),
    );
  }

  final import = ImportLink.tryParse(trimmed);
  if (import != null) {
    return EntryClassification._(
      EntryKind.subscriptionUrl,
      url: import.url,
    );
  }

  // Дальше решает СХЕМА, и только она. Спрашивать причину отказа у парсеров
  // ссылок раньше этого места нельзя: `EnrollLink.parse` отвечает
  // `malformedUrl` на всё, что не разбирается как URI, — то есть на конфиг
  // sing-box, который начинается с `{`. Первая же вставка JSON получала бы
  // «адрес не разбирается: нужен полный URL с хостом» вместо импорта.
  final uri = Uri.tryParse(trimmed);
  final scheme = uri?.scheme.toLowerCase() ?? '';

  // Голая ссылка подписки. Схему http/https здесь НЕ судим строже: http://
  // отвергает сама выборка (`csmSafeExternalUri` в subscription_fetch), и её
  // отказ несёт тот же текст INV-8. Второе место с тем же правилом — это
  // второе место, где правило может разъехаться с первым.
  if ((scheme == 'https' || scheme == 'http') && (uri?.host ?? '').isNotEmpty) {
    return EntryClassification._(EntryKind.subscriptionUrl, url: trimmed);
  }

  // Ссылка НАШЕЙ схемы, которую не принял ни один разбор. Причина известна —
  // назовём её. Без этой ветки `carambaconnect://import?url=http://...` уехал
  // бы в ядро как «конфиг» и вернулся ошибкой разбора YAML вместо одной
  // внятной фразы про http:// (INV-8).
  if (scheme == 'carambaconnect') {
    final refusal = _ourLinkRefusal(trimmed);
    return EntryClassification._(
      EntryKind.refused,
      refusalMessage:
          refusal?.message ??
          'Ссылка Caramba не распознана: в ней нет ни приглашения, ни адреса '
              'подписки.',
    );
  }
  if (scheme == kConnectLinkScheme) {
    // `caramba://` с чужим действием: схема наша, смысла нет.
    return const EntryClassification._(
      EntryKind.refused,
      refusalMessage:
          'Это ссылка Caramba, но не приглашение: в ней нет действия connect. '
          'Попросите оператора прислать ссылку целиком.',
    );
  }

  return const EntryClassification._(EntryKind.configText);
}

/// Причина отказа для ссылки НАШЕЙ схемы. `null` — строка вообще не наша, и
/// объяснять человеку нечего: это либо конфиг, либо чужой адрес.
///
/// Тот же приём, что в `DeepLinkHandler.refusalOf`: [LinkRefusal.notOurLink]
/// пропускается, потому что каждый из двух парсеров так отвечает про ссылку
/// СОСЕДА, и приняв этот ответ за вердикт, мы бы отвергали обе ссылки сразу.
LinkRefusal? _ourLinkRefusal(String raw) {
  for (final r in <LinkRefusal?>[
    EnrollLink.parse(raw).refusal,
    ImportLink.parse(raw).refusal,
  ]) {
    if (r != null && r != LinkRefusal.notOurLink) return r;
  }
  return null;
}

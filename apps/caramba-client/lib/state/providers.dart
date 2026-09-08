import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/auth_tokens.dart' show accessTokenExpiry;
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/subscription_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';

/// Корневые DI-провайдеры: secure storage, prefs, API-клиент и VPN-движок.
/// Остальные нотифаеры (`auth`, `servers`, `subscription`, `vpn`) зависят от них.

/// Платформенное secure storage для JWT, КЛЮЧЁВАННОЕ ПО `pid` активного
/// профиля (02-SPEC.md 1.2).
///
/// Единственная глобальная корзина означала бы, что энроллмент второго
/// оператора затирает сессию первого: изоляция тенантов в CSM/1 это свойство
/// документов И хранилища, и на слое хранения она держится ровно здесь.
/// Профиль без закреплённого корня (legacy-импорт) остаётся в legacy-корзине,
/// потому что тенанта у него нет.
///
/// Провайдер ПЕРЕСОБИРАЕТСЯ при смене активного профиля, и всё, что от него
/// зависит (в первую очередь [apiClientProvider]), пересобирается вместе с ним:
/// клиент, продолжающий носить bearer прежнего оператора, это ровно та ошибка,
/// от которой ключевание и спасает.
final tokenStoreProvider = Provider<TokenStore>((ref) {
  // Следим за pid, а НЕ за профилем целиком, и это не оптимизация.
  //
  // У ConnectionProfile нет сравнения на равенство, поэтому Riverpod считает
  // новым любой его copyWith — снимок замера, выбранный узел, состояние
  // протокола, кэш брендинга. Раньше отсюда шла цепочка: любая запись в профиль
  // пересоздавала хранилище токенов, за ним ApiClient, за ним AuthNotifier;
  // новый нотифаер стартует со стадии «неизвестно», роутер уводит на гостевую
  // главную, потом обратно на автонастройку — а та в конце пишет в профиль
  // результат замера и запускает круг заново. Приложение вечно «подбирало
  // настройки» и не давало ни подключиться, ни выйти.
  //
  // Тенантность меняет ровно pid: на него и подписываемся.
  final pid = ref.watch(
    activeConnectionProfileProvider.select((p) => p?.csm?.pin.pid ?? ''),
  );
  final profile = ref.read(activeConnectionProfileProvider);
  final TokenStore store;
  try {
    store = TokenStore(pid: pid);
  } on ArgumentError {
    // Испорченный pid не имеет права стать отказом старта: сессия просто
    // остаётся в legacy-корзине, и это видно в состоянии профиля.
    return TokenStore();
  }
  if (profile != null && pid.isNotEmpty) {
    // Миграция 06-MIGRATION.md 7.1: единственный старый блоб переезжает в
    // корзину профиля, КОТОРОМУ ОН ПРИНАДЛЕЖИТ. Без этого вызова вошедший
    // пользователь терял сессию от одного обновления приложения, потому что
    // читать её стали бы уже по имени с pid.
    //
    // `soleOwner` утверждается ровно тогда, когда в установке один профиль,
    // способный владеть панельной сессией. С двумя владелец неизвестен, и
    // правильный ответ это отказ переносить, а не догадка: отданная не тому
    // сессия подписывает все запросы этого тенанта чужим bearer'ом.
    final owners = ref
        .read(connectionProfilesProvider)
        .profiles
        .where((p) => p.isPanel)
        .length;
    unawaited(
      store.adoptLegacySession(ownerId: profile.id, soleOwner: owners <= 1),
    );
  }
  return store;
});

/// Локальные несекретные настройки (CoreConfig, AppSettings, first-run, режим
/// туннеля). Инстанс один на приложение; читается только после
/// `appBootProvider` (см. `bootstrap_state.dart`), до этого отдаёт дефолты.
final prefsStoreProvider = Provider<PrefsStore>((ref) => PrefsStore());

/// HTTP-клиент к панели. `onSessionExpired` пробрасывает auth-нотифаеру
/// принудительный логаут, когда refresh окончательно протух.
final apiClientProvider = Provider<ApiClient>((ref) {
  // Панель берётся из активного профиля, а не из константы сборки: приложение
  // не принадлежит ни одному оператору, пока пользователь его не выбрал.
  // kApiBaseUrl остаётся только для брендированной сборки конкретной панели и
  // в публичной сборке пуст.
  // Тоже по полю, а не по объекту: см. комментарий в tokenStoreProvider —
  // подписка на весь профиль превращала любую запись в нём в пересоздание
  // сессии и замыкала цикл перерисовки.
  final panelUrl = ref.watch(
    activeConnectionProfileProvider.select((p) => (p?.panelUrl ?? '').trim()),
  );
  final client = ApiClient(
    tokens: ref.watch(tokenStoreProvider),
    baseUrl: panelUrl.isNotEmpty ? panelUrl : null,
  );
  ref.onDispose(() => client.onSessionExpired = null);
  return client;
});

/// VPN-движок. На всех 5 платформах (Android/iOS/macOS/Windows/Linux) —
/// нативное Go-ядро (mihomo), когда включён флаг `USE_NATIVE_VPN`: на macOS
/// внутрипроцессно через dart:ffi (`libcaramba_core.dylib`, без Xcode и
/// Network Extension), на остальных через каналы `com.caramba/vpn`. Иначе и на
/// web — [MockVpnConnection], чтобы UI работал end-to-end без нативного бэка.
final vpnConnectionProvider = Provider<VpnConnection>((ref) {
  final conn = createVpnConnection(
    native: _useNativeVpn(),
    configResolver: () => _resolveVpnConfig(ref),
  );
  ref.onDispose(conn.dispose);
  return conn;
});

/// Работает ли сейчас нативное ядро (а не мок). Экраны, у которых «настоящий»
/// и «имитационный» пути расходятся (autotune), ветвятся по нему.
final isNativeVpnProvider = Provider<bool>((ref) => _useNativeVpn());

/// Способ захвата трафика по умолчанию для текущей платформы.
///
/// Мобильные строят системный TUN через VpnService/NetworkExtension — там
/// [TunnelMode.tun]. Desktop так не может без прав администратора (а на macOS
/// без Xcode-расширения вовсе), поэтому там [TunnelMode.proxy]: локальный
/// mixed-инбаунд на 127.0.0.1.
TunnelMode defaultTunnelMode() {
  if (kIsWeb) return TunnelMode.proxy;
  if (Platform.isAndroid || Platform.isIOS) return TunnelMode.tun;
  return TunnelMode.proxy;
}

/// Порт локального mixed-инбаунда в [TunnelMode.proxy].
const int kMixedPort = 7890;

/// Выбранный способ захвата трафика. Персистится: пользователь, переключивший
/// desktop на TUN (запустив приложение с правами), не должен делать это заново
/// после каждого перезапуска. Применяется вызовом `setTunnelMode` перед connect.
class TunnelModeNotifier extends StateNotifier<TunnelMode> {
  final PrefsStore? _prefs;

  TunnelModeNotifier(this._prefs, TunnelMode initial) : super(initial);

  /// Ставит значение из [PrefsStore] на старте (не пишет обратно).
  void hydrate(TunnelMode mode) => super.state = mode;

  @override
  set state(TunnelMode value) {
    super.state = value;
    unawaited(_prefs?.writeString(PrefsStore.kTunnelMode, value.wire));
  }

  void set(TunnelMode mode) => state = mode;
}

final tunnelModeProvider =
    StateNotifierProvider<TunnelModeNotifier, TunnelMode>(
      (ref) => TunnelModeNotifier(
        ref.watch(prefsStoreProvider),
        defaultTunnelMode(),
      ),
    );

/// Лениво собирает конфиг для авторизации Go-ядра перед поднятием туннеля
/// (путь panelAccount). Источник зависит от активного профиля подключения:
///   * rawSub          → `null` (configure не нужен, ядро поднимает импорт);
///   * panelAccount со своими полями (panelUrl/subscriptionUuid) → берём их из
///     профиля (мульти-аккаунт, иной тенант), а токен предпочитаем СВЕЖИЙ из
///     [TokenStore]: ApiClient ротирует его при 401, и запись на профиле
///     устаревает уже через час;
///   * профиль не задан или поля пусты → дефолтный путь тенанта-1: JWT из
///     TokenStore + UUID подписки (`GET /subscription`) + [kApiBaseUrl].
/// Возвращает `null`, если данных ещё нет — тогда `configure` пропускается и
/// нативная сторона ответит стадией `error`.
Future<VpnConfig?> _resolveVpnConfig(Ref ref) async {
  final profile = ref.read(activeConnectionProfileProvider);

  // Импортированная подписка авторизации панели не требует.
  if (profile != null && profile.isRaw) return null;

  // panelAccount, несущий собственные креды (например другой тенант): берём их
  // напрямую из профиля, минуя дефолтный путь тенанта-1.
  if (profile != null && profile.isPanel) {
    final panelUrl = profile.panelUrl;
    final uuid = profile.subscriptionUuid;
    // Токен ротируется в общем TokenStore (в т.ч. энроллмент-сессией), поэтому
    // он приоритетнее снимка, лежащего на профиле.
    final store = ref.read(tokenStoreProvider);
    final live = await store.readAccess();
    final token = (live != null && live.isNotEmpty)
        ? live
        : profile.accessToken;
    // refresh берём ТОЛЬКО из общего хранилища: на профиле его снимка нет и
    // быть не должно — он ротируется при каждом обновлении пары.
    final refresh = await store.readRefresh() ?? '';
    if (panelUrl != null &&
        panelUrl.isNotEmpty &&
        uuid != null &&
        uuid.isNotEmpty &&
        token != null &&
        token.isNotEmpty) {
      return VpnConfig(
        panelUrl: panelUrl,
        subscriptionUuid: uuid,
        accessToken: token,
        // Пара из TokenStore принадлежит одной сессии; снимок на профиле — нет.
        // Отдавать чужой refresh к чужому access значило бы дать ядру продлить
        // не ту сессию, поэтому refresh едет только со «свежим» токеном.
        refreshToken: (live != null && live.isNotEmpty) ? refresh : '',
        accessExpiry: accessTokenExpiry(token),
      );
    }
    // Поля профиля неполны — падаем на дефолтный путь тенанта-1 ниже.
  }

  // Дефолтный путь тенанта-1: текущая сессия TokenStore + активная подписка.
  final session = await _resolveSession(ref);
  if (session == null) return null;
  final Subscription sub;
  try {
    sub = await ref.read(subscriptionProvider.future);
  } catch (_) {
    return null;
  }
  if (sub.subscriptionUuid.isEmpty) return null;
  return VpnConfig(
    panelUrl: kApiBaseUrl,
    subscriptionUuid: sub.subscriptionUuid,
    accessToken: session.access,
    refreshToken: session.refresh,
    accessExpiry: session.expiry,
  );
}

/// Живая сессия панели из [TokenStore]: пара токенов и срок жизни access.
///
/// Отдельная функция, потому что сессию читают ДВА пути — подключение и замер —
/// и разойтись им нельзя. Пара всегда берётся целиком: access без своего
/// refresh даёт ядру ровно 15 минут жизни, а refresh от другой сессии дал бы
/// ему продлить не ту.
Future<({String access, String refresh, DateTime? expiry})?> _resolveSession(
  Ref ref,
) async {
  final store = ref.read(tokenStoreProvider);
  final access = await store.readAccess();
  if (access == null || access.isEmpty) return null;
  return (
    access: access,
    refresh: await store.readRefresh() ?? '',
    expiry: accessTokenExpiry(access),
  );
}

/// Резолвер панельного шва ДЛЯ ЗАМЕРА: те же токены, что и у подключения, но
/// без единого сетевого запроса.
///
/// Замер обязан отдать ядру шов до того, как что-то мерить, и раньше собирал
/// его сам из полей профиля — а там лежит СНИМОК токена, сделанный при создании
/// профиля: он устаревает за час и refresh не содержит вовсе, так что замер на
/// отлежавшемся телефоне уходил в ядро с мёртвой сессией.
///
/// Но и [_resolveVpnConfig] сюда не годится: на дефолтном пути он ждёт
/// `GET /subscription`, и замер начинал бы с ожидания сети — при том, что
/// панельный профиль свой UUID уже знает. Поэтому endpoint здесь берётся
/// ТОЛЬКО с профиля, а сессия — из общего хранилища.
final probeSeamResolverProvider = Provider<VpnConfigResolver>(
  (ref) => () async {
    final profile = ref.read(activeConnectionProfileProvider);
    if (profile == null || !profile.isPanel) return null;
    final panelUrl = profile.panelUrl ?? '';
    if (panelUrl.isEmpty) return null;
    final session = await _resolveSession(ref);
    if (session == null) return null;
    return VpnConfig(
      panelUrl: panelUrl,
      subscriptionUuid: profile.subscriptionUuid ?? '',
      accessToken: session.access,
      refreshToken: session.refresh,
      accessExpiry: session.expiry,
    );
  },
);

/// Резолвер панельного шва для ПОДКЛЮЧЕНИЯ — тем же способом, каким его
/// получает [vpnConnectionProvider]. В отличие от [probeSeamResolverProvider]
/// он вправе сходить за UUID подписки в сеть.
final vpnSeamResolverProvider = Provider<VpnConfigResolver>(
  (ref) => () => _resolveVpnConfig(ref),
);

/// Включает нативное Go-ядро вместо [MockVpnConnection]. По умолчанию выключено,
/// чтобы голый `flutter run` (CI/dev без собранных нативных либ) показывал UI на
/// моке и не ловил MissingPluginException на незарегистрированном канале.
/// Поднимать флагом сборки: `flutter run --dart-define=USE_NATIVE_VPN=true`.
const bool _nativeVpnEnabled = bool.fromEnvironment(
  'USE_NATIVE_VPN',
  defaultValue: false,
);

/// Нативный путь доступен на всех 5 десктоп/мобильных платформах (плагин
/// `caramba_vpn` регистрирует `com.caramba/vpn` на каждой, а macOS идёт по
/// dart:ffi). Web — всегда мок. Гейтится флагом [_nativeVpnEnabled].
bool _useNativeVpn() {
  if (kIsWeb) return false;
  if (!_nativeVpnEnabled) return false;
  return Platform.isAndroid ||
      Platform.isIOS ||
      Platform.isMacOS ||
      Platform.isWindows ||
      Platform.isLinux;
}

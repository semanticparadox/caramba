import 'dart:async';

import 'package:dio/dio.dart';

import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/data/models/notification.dart';
import 'package:caramba_client/data/models/partner.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/ticket.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/data/models/traffic_point.dart';
import 'package:caramba_client/data/models/user.dart';
import 'package:caramba_client/data/token_store.dart';

/// Базовый URL панели. Переопределяется через `--dart-define=CARAMBA_API_BASE=...`.
/// По умолчанию — локальная панель в dev (Android-эмулятор: 10.0.2.2).
const String kApiBaseUrl = String.fromEnvironment(
  'CARAMBA_API_BASE',
  defaultValue: 'https://exarobot.top',
);

/// Исключение уровня API с человекочитаемым сообщением и HTTP-кодом.
class ApiException implements Exception {
  final String message;
  final int? statusCode;

  const ApiException(this.message, {this.statusCode});

  /// Истёкла/невалидна сессия — UI должен разлогинить пользователя.
  bool get isUnauthorized => statusCode == 401;

  @override
  String toString() => 'ApiException($statusCode): $message';
}

/// HTTP-клиент к панели (`/api/v2/app/*`).
///
/// Отвечает за:
///   * добавление `Authorization: Bearer <access>` к защищённым запросам;
///   * прозрачный авто-refresh при 401 (single-flight: параллельные 401
///     ждут одну ротацию) с повтором исходного запроса;
///   * (де)сериализацию в модели.
///
/// Контракт зеркалит `apps/caramba-panel/src/api/v2/app_auth.rs` и `app.rs`.
class ApiClient {
  final Dio _dio;
  final TokenStore _tokens;

  /// Колбэк, вызываемый когда refresh окончательно провалился — auth-слой
  /// подписывается на него, чтобы перевести сессию в `unauthenticated`.
  void Function()? onSessionExpired;

  /// Single-flight: текущая операция ротации refresh-токена.
  Future<AuthTokens?>? _refreshing;

  ApiClient({required TokenStore tokens, Dio? dio, String? baseUrl})
    : _tokens = tokens,
      _dio =
          dio ??
          Dio(
            BaseOptions(
              baseUrl: '${baseUrl ?? kApiBaseUrl}/api/v2/app',
              connectTimeout: const Duration(seconds: 15),
              receiveTimeout: const Duration(seconds: 20),
              contentType: Headers.jsonContentType,
              // Сами решаем по статус-коду — не бросаем на не-2xx автоматически.
              validateStatus: (s) => s != null && s < 500,
            ),
          ) {
    _dio.interceptors.add(
      InterceptorsWrapper(
        onRequest: (options, handler) async {
          if (options.extra['skipAuth'] != true) {
            final access = await _tokens.readAccess();
            if (access != null && access.isNotEmpty) {
              options.headers['Authorization'] = 'Bearer $access';
            }
          }
          handler.next(options);
        },
        onResponse: (response, handler) async {
          // 401 на защищённом запросе → пробуем один авто-refresh + повтор.
          final req = response.requestOptions;
          if (response.statusCode == 401 &&
              req.extra['skipAuth'] != true &&
              req.extra['retried'] != true) {
            final newTokens = await _refreshTokens();
            if (newTokens != null) {
              final retried = await _retry(req, newTokens.accessToken);
              return handler.resolve(retried);
            }
          }
          handler.next(response);
        },
      ),
    );
  }

  // ------------------------------------------------------------------
  // Auth (публичные эндпоинты, skipAuth)
  // ------------------------------------------------------------------

  /// POST /register — регистрация по email/password. Панель отвечает 201 + пара.
  ///
  /// [enrollCode] (P2, опц.) — инвайт-код энроллмента. Если задан, панель в
  /// одной транзакции при создании аккаунта валидирует код, расходует одно
  /// использование (used_count++ под row-lock), проставляет signup-source и,
  /// если онбординг включён, единоразово начисляет онбординг-трафик. Поле в
  /// теле — `enroll_code` (зеркалит `RegisterRequest.enroll_code` в
  /// `app_auth.rs`). Отсутствие поля => прежнее поведение без энроллмента.
  Future<AuthTokens> register({
    required String email,
    required String password,
    String? fullName,
    String? enrollCode,
  }) async {
    final res = await _dio.post(
      '/register',
      data: {
        'email': email,
        'password': password,
        if (fullName != null && fullName.isNotEmpty) 'full_name': fullName,
        if (enrollCode != null && enrollCode.isNotEmpty)
          'enroll_code': enrollCode,
      },
      options: Options(extra: {'skipAuth': true}),
    );
    return _tokensFrom(res);
  }

  /// GET /enroll/{code} — ПУБЛИЧНАЯ валидация enroll-кода (P2, contract B).
  ///
  /// Без JWT (`skipAuth`). Чистое чтение: код НЕ расходуется. Возвращает
  /// валидность, имя панели и размер разового онбординг-трафика. Контракт:
  /// `{ valid, reason?, panel_name?, onboarding_traffic_mb }`. PII не отдаётся.
  ///
  /// Этот клиент должен быть нацелен на URL панели из ссылки (см.
  /// `ApiClient(baseUrl: panelUrl)`), а не на дефолтный тенант-1.
  Future<EnrollValidation> validateEnroll(String code) async {
    final res = await _dio.get(
      '/enroll/${Uri.encodeComponent(code)}',
      options: Options(extra: {'skipAuth': true}),
    );
    return EnrollValidation.fromJson(_okMap(res));
  }

  /// POST /login/email — вход по email/password.
  Future<AuthTokens> loginEmail({
    required String email,
    required String password,
  }) async {
    final res = await _dio.post(
      '/login/email',
      data: {'email': email, 'password': password},
      options: Options(extra: {'skipAuth': true}),
    );
    return _tokensFrom(res);
  }

  /// POST /login/telegram — вход через Telegram.
  ///
  /// Передаётся либо сырой [initData] (Telegram WebApp), либо плоские поля
  /// Login Widget ([id], [hash], [authDate], ...).
  Future<AuthTokens> loginTelegram({
    String? initData,
    int? id,
    String? firstName,
    String? lastName,
    String? username,
    int? authDate,
    String? hash,
  }) async {
    final res = await _dio.post(
      '/login/telegram',
      data: {
        if (initData != null) 'init_data': initData,
        if (id != null) 'id': id,
        if (firstName != null) 'first_name': firstName,
        if (lastName != null) 'last_name': lastName,
        if (username != null) 'username': username,
        if (authDate != null) 'auth_date': authDate,
        if (hash != null) 'hash': hash,
      },
      options: Options(extra: {'skipAuth': true}),
    );
    return _tokensFrom(res);
  }

  /// POST /login/code — вход по коду из Telegram-бота (6 цифр). Панель сверяет
  /// код, привязанный к Telegram-аккаунту, и выдаёт JWT-пару.
  ///
  /// [enrollCode] (P2, опц.) — enroll-код энроллмента. login/code НЕ создаёт
  /// свежий аккаунт (резолвит уже существующий tg_id, заведённый ботом), так
  /// что панель его расходует только если за этим входом стоит создание нового
  /// аккаунта; иначе это no-op. Шлём как `enroll_code`.
  Future<AuthTokens> loginCode({
    required String code,
    String? enrollCode,
  }) async {
    final res = await _dio.post(
      '/login/code',
      data: {
        'code': code,
        if (enrollCode != null && enrollCode.isNotEmpty)
          'enroll_code': enrollCode,
      },
      options: Options(extra: {'skipAuth': true}),
    );
    return _tokensFrom(res);
  }

  /// POST /logout — отзыв refresh-токена на сервере (идемпотентно).
  Future<void> logout(String refreshToken) async {
    try {
      await _dio.post(
        '/logout',
        data: {'refresh_token': refreshToken},
        options: Options(extra: {'skipAuth': true}),
      );
    } catch (_) {
      // Logout best-effort: локальную очистку делает auth-слой в любом случае.
    }
  }

  /// GET /branding — ПУБЛИЧНЫЙ брендинг инстанса панели (P3, contract A/E).
  ///
  /// Без JWT (`skipAuth`) — нужен ещё до логина. Чистое чтение. Контракт:
  /// `{ enabled, brand_name, logo_url, accent_hex, support_url, bot_url,
  ///    upstream_ads }`. Гейт тира делает панель (Free => enabled=false,
  /// upstream_ads=true). Этот клиент должен быть нацелен на URL АКТИВНОГО
  /// `panelAccount`-профиля (см. `ApiClient(baseUrl: profile.panelUrl)`), а не
  /// на дефолтного тенанта-1.
  ///
  /// Любая ошибка/малформед => дефолт Caramba Connect ([Branding.fallback]):
  /// бренд выключен, upsell включён. Брендинг не должен ронять login/connect.
  Future<Branding> getBranding() async {
    try {
      final res = await _dio.get(
        '/branding',
        options: Options(extra: {'skipAuth': true}),
      );
      if ((res.statusCode ?? 0) >= 400) return Branding.fallback;
      final data = res.data;
      if (data is! Map) return Branding.fallback;
      return Branding.fromJson(data.cast<String, dynamic>());
    } catch (_) {
      return Branding.fallback;
    }
  }

  // ------------------------------------------------------------------
  // Protected (требуют Bearer; авто-refresh в интерсепторе)
  // ------------------------------------------------------------------

  /// GET /me — профиль пользователя.
  Future<User> getMe() async {
    final res = await _dio.get('/me');
    return User.fromJson(_okMap(res));
  }

  /// GET /subscription — активная подписка + URL mihomo/clash-конфига.
  Future<Subscription> getSubscription() async {
    final res = await _dio.get('/subscription');
    return Subscription.fromJson(_okMap(res));
  }

  /// GET /servers — список доступных exit-серверов.
  Future<List<Server>> getServers() async {
    final res = await _dio.get('/servers');
    _ensureOk(res);
    final data = res.data;
    if (data is! List) {
      throw const ApiException('Malformed servers response');
    }
    return data
        .whereType<Map>()
        .map((e) => Server.fromJson(e.cast<String, dynamic>()))
        .toList(growable: false);
  }

  // ------------------------------------------------------------------
  // Account: devices / subscriptions / referrals / family / relays
  // ------------------------------------------------------------------

  /// GET /devices — все устройства (lease) по подпискам пользователя.
  /// Контракт: `app_account.rs::list_devices` (AppDevice[]).
  Future<List<Device>> getDevices() async {
    final res = await _dio.get('/devices');
    return _list(res, Device.fromJson, 'devices');
  }

  /// PATCH /devices/{id} — переименование устройства. Пустое/`null` имя
  /// сбрасывает на авто-имя. Возвращает `true` при успехе.
  Future<bool> renameDevice(int id, String? name) async {
    final res = await _dio.patch('/devices/$id', data: {'name': name});
    _ensureOk(res);
    return true;
  }

  /// DELETE /devices/{id} — отзыв (kick) устройства.
  Future<bool> removeDevice(int id) async {
    final res = await _dio.delete('/devices/$id');
    _ensureOk(res);
    return true;
  }

  /// GET /subscriptions — список подписок пользователя для профиля/Home.
  /// Контракт: `app_account.rs::list_subscriptions` (AppSubscription[]).
  Future<List<SubPlan>> getSubscriptions() async {
    final res = await _dio.get('/subscriptions');
    return _list(res, SubPlan.fromJson, 'subscriptions');
  }

  /// GET /referrals — реферальная сводка: код, ссылка, приглашённые, текущий
  /// баланс и всего начислено (минорные единицы), проценты вознаграждения/скидки
  /// и список приглашённых. Контракт: `app_account.rs::get_referrals`
  /// (AppReferrals; денежная модель — баланс рефереру, скидка приглашённому).
  Future<ReferralInfo> getReferrals() async {
    final res = await _dio.get('/referrals');
    return ReferralInfo.fromJson(_okMap(res));
  }

  /// GET /family — участники семьи. [subscriptionId] (опц.) валидирует владение
  /// конкретной подпиской. Контракт: `app_account.rs::get_family` (FamilyResponse).
  Future<Family> getFamily({int? subscriptionId}) async {
    final res = await _dio.get(
      '/family',
      queryParameters: {
        if (subscriptionId != null) 'subscription_id': subscriptionId,
      },
    );
    return Family.fromJson(_okMap(res));
  }

  /// POST /family/invite — создаёт инвайт в семью. Возвращает код и срок.
  /// Контракт: `app_account.rs::create_family_invite` (FamilyInviteResponse).
  Future<FamilyInvite> inviteFamily({
    int? subscriptionId,
    int? maxUses,
    int? durationDays,
  }) async {
    final res = await _dio.post(
      '/family/invite',
      data: {
        if (subscriptionId != null) 'subscription_id': subscriptionId,
        if (maxUses != null) 'max_uses': maxUses,
        if (durationDays != null) 'duration_days': durationDays,
      },
    );
    return FamilyInvite.fromJson(_okMap(res));
  }

  /// DELETE /family/{memberId} — исключить участника из семьи.
  Future<bool> removeFamilyMember(int memberId) async {
    final res = await _dio.delete('/family/$memberId');
    _ensureOk(res);
    return true;
  }

  /// GET /relays — relay-страны для пикера входа. Контракт:
  /// `app_account.rs::list_relays` (AppRelay[]). Спец-варианты Выкл/Авто
  /// клиент добавляет сам (см. [Relay.fromCountries]).
  Future<List<Relay>> getRelays() async {
    final res = await _dio.get('/relays');
    return _list(res, Relay.fromApiJson, 'relays');
  }

  /// GET /traffic — подневная история трафика (~30 дней) для графика.
  ///
  /// Контракт: `app_billing.rs::get_traffic` — объект
  /// `{ points: [{ date: "YYYY-MM-DD", up_bytes, down_bytes, total_bytes }],
  ///    total_up, total_down, cumulative_used_bytes, direction_split }`.
  /// Берём только `points`; на любую ошибку — пустой список (UI рисует «нет данных»).
  Future<List<TrafficPoint>> getTraffic() async {
    try {
      final res = await _dio.get('/traffic');
      if ((res.statusCode ?? 0) >= 400) return const [];
      final data = res.data;
      if (data is! Map) return const [];
      final points = data['points'];
      if (points is! List) return const [];
      return points
          .whereType<Map>()
          .map((e) => TrafficPoint.fromJson(e.cast<String, dynamic>()))
          .toList(growable: false);
    } catch (_) {
      return const [];
    }
  }

  /// POST /purchase — чек-аут плана. Контракт: `app_billing.rs::purchase`
  /// (`{ duration_id, provider? }` -> `{ pay_url, session_id, amount, currency,
  /// fulfilled }`). [durationId] — это `plan_durations.id`.
  ///
  /// Возвращает `pay_url` для открытия во внешнем браузере; для оплаты с баланса
  /// панель отдаёт `"SUCCESS"` + `fulfilled=true` — тогда возвращаем `null`
  /// (открывать нечего, подписка уже активирована). Текущий путь покупки в UI —
  /// deeplink в бота (ProfileScreen); этот метод оставлен под in-app pay_url.
  Future<String?> purchase(int durationId, {String? provider}) async {
    final res = await _dio.post(
      '/purchase',
      data: {
        'duration_id': durationId,
        if (provider != null && provider.isNotEmpty) 'provider': provider,
      },
    );
    if ((res.statusCode ?? 0) >= 400) {
      throw ApiException(_messageOf(res), statusCode: res.statusCode);
    }
    final data = res.data;
    if (data is Map) {
      final m = data.cast<String, dynamic>();
      if (m['fulfilled'] == true) return null;
      final url = m['pay_url'];
      if (url is String && url.isNotEmpty && url != 'SUCCESS') return url;
    }
    return null;
  }

  // ------------------------------------------------------------------
  // Notifications (`/app/notifications`)
  // ------------------------------------------------------------------

  /// GET /notifications — лента уведомлений + авторитетный `unread_count`.
  ///
  /// Панель отдаёт `{ notifications: [...], unread_count }`. На случай иной
  /// обёртки терпим и голый массив, и ключ `items` (см. [_listFlex]); тогда
  /// серверного счётчика нет и клиент считает локально.
  Future<NotificationsPage> getNotifications() async {
    final res = await _dio.get('/notifications');
    final items = _listFlex(
      res,
      AppNotification.fromJson,
      'notifications',
      keys: const ['items', 'notifications'],
    );
    final data = res.data;
    final unreadCount = (data is Map)
        ? (data['unread_count'] as num?)?.toInt()
        : null;
    return NotificationsPage(items: items, unreadCount: unreadCount);
  }

  /// POST /notifications/{id}/read — пометить одно уведомление прочитанным.
  Future<bool> markNotificationRead(int id) async {
    final res = await _dio.post('/notifications/$id/read');
    _ensureOk(res);
    return true;
  }

  /// POST /notifications/read-all — пометить все уведомления прочитанными.
  Future<bool> markAllNotificationsRead() async {
    final res = await _dio.post('/notifications/read-all');
    _ensureOk(res);
    return true;
  }

  // ------------------------------------------------------------------
  // Support tickets (`/app/tickets`)
  // ------------------------------------------------------------------

  /// GET /tickets — список тикетов пользователя (`TicketSummary[]`).
  Future<List<TicketSummary>> getTickets() async {
    final res = await _dio.get('/tickets');
    return _listFlex(
      res,
      TicketSummary.fromJson,
      'tickets',
      keys: const ['items', 'tickets'],
    );
  }

  /// POST /tickets — создать тикет. Контракт: `{ subject, message }` ->
  /// созданный тикет (минимум `{ id }`). Возвращает id нового тикета.
  Future<int> createTicket({
    required String subject,
    required String message,
  }) async {
    final res = await _dio.post(
      '/tickets',
      data: {'subject': subject, 'message': message},
    );
    final m = _okMap(res);
    final t = (m['ticket'] is Map)
        ? (m['ticket'] as Map).cast<String, dynamic>()
        : m;
    return (t['id'] as num?)?.toInt() ?? 0;
  }

  /// GET /tickets/{id} — тикет с лентой сообщений (`TicketDetail`).
  Future<TicketDetail> getTicket(int id) async {
    final res = await _dio.get('/tickets/$id');
    return TicketDetail.fromJson(_okMap(res));
  }

  /// POST /tickets/{id}/reply — ответ в тикет. Контракт: `{ message }`.
  /// Возвращает созданное сообщение, если панель его отдаёт (иначе null).
  Future<TicketMessage?> replyTicket(int id, String message) async {
    final res = await _dio.post(
      '/tickets/$id/reply',
      data: {'message': message},
    );
    _ensureOk(res);
    final data = res.data;
    if (data is Map) {
      final m = data.cast<String, dynamic>();
      final msg = (m['message'] is Map)
          ? (m['message'] as Map).cast<String, dynamic>()
          : m;
      if (msg['body'] != null || msg['id'] != null) {
        return TicketMessage.fromJson(msg);
      }
    }
    return null;
  }

  // ------------------------------------------------------------------
  // Partner (`/app/partner/*`, гейтится партнёрской ролью на панели)
  // ------------------------------------------------------------------

  /// GET /partner/codes — партнёрская сводка: флаг роли + коды с постатейной
  /// статистикой (клики/регистрации/конверсии/начислено). Контракт:
  /// `app_partner.rs::list_partner_codes` (PartnerOverview). Не-партнёру панель
  /// отдаёт `{ is_partner: false, codes: [] }` — UI прячет раздел.
  Future<PartnerOverview> getPartnerCodes() async {
    final res = await _dio.get('/partner/codes');
    return PartnerOverview.fromJson(_okMap(res));
  }

  /// POST /partner/codes — создать партнёрский код для источника. Контракт:
  /// `{ source_label }` -> созданный объект кода. [sourceLabel] — youtube,
  /// tg-канал, имя блогера.
  Future<PartnerCode> createPartnerCode(String sourceLabel) async {
    final res = await _dio.post(
      '/partner/codes',
      data: {'source_label': sourceLabel},
    );
    final m = _okMap(res);
    final c = (m['code'] is Map)
        ? (m['code'] as Map).cast<String, dynamic>()
        : m;
    return PartnerCode.fromJson(c);
  }

  /// DELETE /partner/codes/{code} — удалить партнёрский код. Возвращает `true`
  /// при успехе.
  Future<bool> deletePartnerCode(String code) async {
    final res = await _dio.delete('/partner/codes/$code');
    _ensureOk(res);
    return true;
  }

  /// Парсит ответ, который может быть либо голым массивом, либо объектом с
  /// одним из [keys]-полей-массивов. Используется для notifications/tickets,
  /// где точная обёртка панели не зафиксирована на момент сборки.
  List<T> _listFlex<T>(
    Response<dynamic> res,
    T Function(Map<String, dynamic>) fromJson,
    String what, {
    required List<String> keys,
  }) {
    _ensureOk(res);
    final data = res.data;
    List? list;
    if (data is List) {
      list = data;
    } else if (data is Map) {
      for (final k in keys) {
        if (data[k] is List) {
          list = data[k] as List;
          break;
        }
      }
    }
    if (list == null) throw ApiException('Malformed $what response');
    return list
        .whereType<Map>()
        .map((e) => fromJson(e.cast<String, dynamic>()))
        .toList(growable: false);
  }

  /// Парсит JSON-массив объектов в список моделей. [what] идёт в текст ошибки.
  List<T> _list<T>(
    Response<dynamic> res,
    T Function(Map<String, dynamic>) fromJson,
    String what,
  ) {
    _ensureOk(res);
    final data = res.data;
    if (data is! List) {
      throw ApiException('Malformed $what response');
    }
    return data
        .whereType<Map>()
        .map((e) => fromJson(e.cast<String, dynamic>()))
        .toList(growable: false);
  }

  // ------------------------------------------------------------------
  // Refresh (single-flight)
  // ------------------------------------------------------------------

  /// Ротирует refresh-токен через `/refresh`, сохраняет новую пару.
  /// Параллельные 401 разделяют одну операцию. Возвращает `null` при провале
  /// (тогда дёргается [onSessionExpired]).
  Future<AuthTokens?> _refreshTokens() {
    return _refreshing ??= _doRefresh().whenComplete(() => _refreshing = null);
  }

  Future<AuthTokens?> _doRefresh() async {
    final refresh = await _tokens.readRefresh();
    if (refresh == null || refresh.isEmpty) {
      onSessionExpired?.call();
      return null;
    }
    try {
      final res = await _dio.post(
        '/refresh',
        data: {'refresh_token': refresh},
        options: Options(extra: {'skipAuth': true}),
      );
      if (res.statusCode != 200 || res.data is! Map) {
        await _tokens.clear();
        onSessionExpired?.call();
        return null;
      }
      final tokens = AuthTokens.fromJson(
        (res.data as Map).cast<String, dynamic>(),
      );
      await _tokens.save(tokens);
      return tokens;
    } catch (_) {
      await _tokens.clear();
      onSessionExpired?.call();
      return null;
    }
  }

  /// Повторяет исходный запрос с новым access-токеном (помечен `retried`).
  Future<Response<dynamic>> _retry(RequestOptions req, String access) {
    final opts = Options(
      method: req.method,
      headers: {...req.headers, 'Authorization': 'Bearer $access'},
      extra: {...req.extra, 'retried': true},
      responseType: req.responseType,
      contentType: req.contentType,
    );
    return _dio.request<dynamic>(
      req.path,
      data: req.data,
      queryParameters: req.queryParameters,
      options: opts,
    );
  }

  // ------------------------------------------------------------------
  // Helpers
  // ------------------------------------------------------------------

  AuthTokens _tokensFrom(Response<dynamic> res) {
    _ensureOk(res);
    if (res.data is! Map) throw const ApiException('Malformed token response');
    return AuthTokens.fromJson((res.data as Map).cast<String, dynamic>());
  }

  Map<String, dynamic> _okMap(Response<dynamic> res) {
    _ensureOk(res);
    final data = res.data;
    if (data is! Map) throw const ApiException('Malformed response');
    return data.cast<String, dynamic>();
  }

  void _ensureOk(Response<dynamic> res) {
    final code = res.statusCode ?? 0;
    if (code >= 200 && code < 300) return;
    throw ApiException(_messageOf(res), statusCode: code);
  }

  /// Панель на ошибках отдаёт plain-text body (`(StatusCode, "msg")`), реже JSON.
  String _messageOf(Response<dynamic> res) {
    final data = res.data;
    if (data is String && data.isNotEmpty) return data;
    if (data is Map) {
      final m = data['message'] ?? data['error'];
      if (m is String && m.isNotEmpty) return m;
    }
    return 'Request failed (${res.statusCode})';
  }
}

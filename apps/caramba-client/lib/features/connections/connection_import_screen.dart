import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Формат входных данных импорта. Соответствует контракту Go `subimport.Import`
/// (Build A): auto/clash/singbox/v2ray/uri. По умолчанию [auto] — детект.
enum ImportFormat {
  auto('auto', 'Авто', 'Определить формат'),
  clash('clash', 'Clash', 'mihomo / clash YAML'),
  singbox('singbox', 'sing-box', 'sing-box JSON'),
  v2ray('v2ray', 'v2ray', 'base64-список'),
  uri('uri', 'URI', 'vless:// vmess:// ss:// ...');

  final String wire;
  final String label;
  final String desc;
  const ImportFormat(this.wire, this.label, this.desc);
}

/// Локальная ошибка шага загрузки подписки. Ловится в [_submit] и
/// конвертируется в человекочитаемый inline-текст.
class ImportException implements Exception {
  final String message;
  const ImportException(this.message);
  @override
  String toString() => 'ImportException: $message';
}

/// Экран импорта подписки: вставка URL или сырого текста + выбор формата
/// (auto по умолчанию). Создаёт [ConnectionProfile] типа [ProfileType.rawSub]
/// и делает его активным. Если вставлена ссылка — тело подписки загружается
/// здесь и сохраняется в `rawConfig` (нативный subimport.Import парсит только
/// байты, своего HTTP-клиента не имеет). Парсинг/туннель из сохранённого
/// конфига выполняет нативная сторона (Build C через `importRawProfile`).
///
/// Аффордансы QR-скана и выбора файла — заглушки (без нативной зависимости):
/// показывают тост-подсказку. Ввод остаётся ручным (вставка), что покрывает
/// acceptance на моке/статике.
class ConnectionImportScreen extends ConsumerStatefulWidget {
  const ConnectionImportScreen({super.key});

  @override
  ConsumerState<ConnectionImportScreen> createState() =>
      _ConnectionImportScreenState();
}

class _ConnectionImportScreenState
    extends ConsumerState<ConnectionImportScreen> {
  final _nameController = TextEditingController();
  final _sourceController = TextEditingController();
  ImportFormat _format = ImportFormat.auto;
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _nameController.dispose();
    _sourceController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;

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
              'Импорт подписки',
              trailing: IconBtn(Lucide.arrowLeft, onTap: () => _close(context)),
            ),
            Text(
              'Вставьте ссылку на подписку или сам конфиг. Формат можно оставить '
              'на авто.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s5),

            // Имя профиля (опционально).
            SectionTitle('Имя'),
            TextField(
              controller: _nameController,
              enabled: !_busy,
              style: AppType.bodyMd.copyWith(color: c.textHi),
              decoration: const InputDecoration(
                hintText: 'Например, «Резерв» (необязательно)',
              ),
            ),
            const SizedBox(height: AppSpace.s5),

            // Источник: URL или сырой конфиг.
            SectionTitle('Ссылка или конфиг'),
            TextField(
              controller: _sourceController,
              enabled: !_busy,
              minLines: 3,
              maxLines: 8,
              style: AppType.monoMd.copyWith(color: c.textHi),
              onChanged: (_) {
                if (_error != null) setState(() => _error = null);
              },
              decoration: const InputDecoration(
                hintText: 'https://... или vless://... или YAML/JSON',
              ),
            ),
            const SizedBox(height: AppSpace.s3),

            // Аффордансы QR / файл (заглушки, без нативной зависимости).
            Row(
              children: [
                Expanded(
                  child: GhostButton(
                    label: 'Сканировать QR',
                    icon: Lucide.appWindow,
                    onPressed: _busy ? null : _scanQrStub,
                  ),
                ),
                const SizedBox(width: AppSpace.s3),
                Expanded(
                  child: GhostButton(
                    label: 'Из файла',
                    icon: Lucide.inbox,
                    onPressed: _busy ? null : _pickFileStub,
                  ),
                ),
              ],
            ),
            const SizedBox(height: AppSpace.s5),

            // Выбор формата (auto по умолчанию).
            SectionTitle('Формат'),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.layers,
                  label: 'Формат',
                  value: _format.label,
                  chevron: true,
                  onTap: _busy ? null : _pickFormat,
                ),
              ],
            ),
            const SizedBox(height: AppSpace.s2),
            Text(
              _format.desc,
              style: AppType.bodySm.copyWith(color: c.textLow),
            ),
            const SizedBox(height: AppSpace.s6),

            if (_error != null) ...[
              InlineError(message: _error!, onRetry: _submit),
              const SizedBox(height: AppSpace.s5),
            ],

            FilledButton.icon(
              onPressed: _busy ? null : _submit,
              icon: _busy
                  ? SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                        strokeWidth: 2,
                        color: c.textOnAccent,
                      ),
                    )
                  : LucideIcon(Lucide.check, color: c.textOnAccent, size: 18),
              label: Text(_busy ? 'Импорт' : 'Импортировать'),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _pickFormat() async {
    final values = ImportFormat.values;
    final selected = await showPickerSheet(
      context: context,
      title: 'Формат импорта',
      subtitle: 'Авто подходит для большинства подписок.',
      selected: values.indexOf(_format),
      options: [
        for (final f in values)
          (name: f.label, desc: f.desc, icon: Lucide.layers),
      ],
    );
    if (selected != null && mounted) {
      setState(() => _format = values[selected]);
    }
  }

  Future<void> _submit() async {
    final source = _sourceController.text.trim();
    if (source.isEmpty) {
      setState(() => _error = 'Вставьте ссылку или конфиг.');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });

    try {
      final now = DateTime.now().millisecondsSinceEpoch;
      final looksLikeUrl =
          source.startsWith('http://') || source.startsWith('https://');
      // Ссылку на подписку тянем здесь и сохраняем тело в rawConfig: нативная
      // сторона (subimport.Import) умеет только парсить байты, HTTP-клиента у
      // неё нет. Так raw всегда несёт реальный конфиг, а не голый URL.
      final rawConfig = looksLikeUrl
          ? await _fetchSubscription(source)
          : source;
      final profile = ConnectionProfile(
        id: 'cp_$now',
        type: ProfileType.rawSub,
        displayName: _defaultName(source),
        source: source,
        rawConfig: rawConfig,
        lastActiveMs: 0,
      );
      await ref.read(connectionProfilesProvider.notifier).add(profile);
      if (!mounted) return;
      showCarambaToast(context, 'Профиль добавлен');
      _close(context);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = 'Не удалось импортировать. Проверьте ссылку или конфиг.';
      });
    }
  }

  /// Загружает тело подписки по ссылке. Используется отдельный [Dio] без
  /// панельного baseUrl и заголовка Authorization: подписка — произвольный
  /// внешний URL, не эндпоинт панели. Бросает при пустом ответе или сетевой
  /// ошибке — её ловит [_submit] и показывает inline-ошибку.
  Future<String> _fetchSubscription(String url) async {
    final dio = Dio(
      BaseOptions(
        connectTimeout: const Duration(seconds: 15),
        receiveTimeout: const Duration(seconds: 20),
        responseType: ResponseType.plain,
        followRedirects: true,
      ),
    );
    try {
      final res = await dio.get<String>(url);
      final body = res.data?.trim() ?? '';
      if (res.statusCode == null ||
          res.statusCode! < 200 ||
          res.statusCode! >= 300 ||
          body.isEmpty) {
        throw const ImportException('пустой ответ подписки');
      }
      return body;
    } finally {
      dio.close();
    }
  }

  String _defaultName(String source) {
    final typed = _nameController.text.trim();
    if (typed.isNotEmpty) return typed;
    final uri = Uri.tryParse(source);
    if (uri != null && uri.host.isNotEmpty) return uri.host;
    return 'Подписка';
  }

  void _scanQrStub() {
    showCarambaToast(
      context,
      'Сканирование QR появится позже. Пока вставьте ссылку вручную.',
    );
  }

  void _pickFileStub() {
    showCarambaToast(
      context,
      'Импорт из файла появится позже. Пока вставьте конфиг текстом.',
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.connections);
    }
  }
}

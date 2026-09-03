/// Единственная точка, через которую приложение открывает ссылку, пришедшую
/// снаружи.
///
/// Допускаются ровно `https` и `mailto`, плюс `http` на `.onion`, потому что
/// луковые адреса самоаутентичны (INV-8, 02-SPEC.md 8.10). Всё остальное,
/// включая `javascript:`, `file:`, `intent:` и любую собственную схему чужого
/// приложения, отвергается: значение приходит по неподписанному маршруту, и
/// «оно же от нашего сервера» здесь не проверяемое утверждение.
Uri? csmSafeExternalUri(String raw) {
  final uri = Uri.tryParse(raw.trim());
  if (uri == null || !uri.hasScheme) {
    return null;
  }
  final scheme = uri.scheme.toLowerCase();
  if (scheme == 'mailto') {
    return uri.path.isEmpty ? null : uri;
  }
  if (uri.host.isEmpty) {
    return null;
  }
  final isOnion = uri.host.toLowerCase().endsWith('.onion');
  if (scheme == 'https' || (scheme == 'http' && isOnion)) {
    return uri;
  }
  return null;
}

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:caramba_client/desktop/ports/autostart_port.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('launch_at_startup');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  setUp(() => debugDefaultTargetPlatformOverride = TargetPlatform.macOS);
  tearDown(() {
    debugDefaultTargetPlatformOverride = null;
    messenger.setMockMethodCallHandler(channel, null);
  });

  test('macOS 12 unsupported does not expose an operable switch', () async {
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(code: kAutostartUnsupportedCode);
    });
    expect(await LaunchAtStartupPort().isSupported(), isFalse);
  });

  test('missing native channel is not classified as an old macOS version',
      () async {
    await expectLater(
      LaunchAtStartupPort().isSupported(),
      throwsA(isA<MissingPluginException>()),
    );
  });

  test('unexpected native failure remains distinguishable from unsupported',
      () async {
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(code: 'failed');
    });
    await expectLater(
      LaunchAtStartupPort().isSupported(),
      throwsA(isA<PlatformException>()),
    );
  });

  test('pending approval supports autostart but is not enabled', () async {
    messenger.setMockMethodCallHandler(channel, (_) async {
      throw PlatformException(code: kAutostartApprovalCode);
    });
    final port = LaunchAtStartupPort();
    expect(await port.isSupported(), isTrue);
    await expectLater(port.isEnabled(), throwsA(isA<PlatformException>()));
  });

  test('disable cancels pending registration without enabled preflight',
      () async {
    final calls = <MethodCall>[];
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      if (call.method == 'launchAtStartupIsEnabled') return false;
      return null;
    });
    await LaunchAtStartupPort().disable();
    expect(calls, hasLength(1));
    expect(calls.single.method, 'launchAtStartupSetEnabled');
    expect(calls.single.arguments, {'setEnabledValue': false});
  });
}

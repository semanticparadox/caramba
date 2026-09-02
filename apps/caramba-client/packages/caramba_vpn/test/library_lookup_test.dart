import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('carambaCoreLibFileName', () {
    test('picks the platform file name', () {
      expect(
        carambaCoreLibFileName(isMacOS: true, isWindows: false),
        'libcaramba_core.dylib',
      );
      expect(
        carambaCoreLibFileName(isMacOS: false, isWindows: true),
        'caramba_core.dll',
      );
      expect(
        carambaCoreLibFileName(isMacOS: false, isWindows: false),
        'libcaramba_core.so',
      );
    });
  });

  group('carambaCoreLibraryCandidates', () {
    test('honours CARAMBA_CORE_LIB first', () {
      final candidates = carambaCoreLibraryCandidates(
        envOverride: '/opt/core/libcaramba_core.dylib',
        executableDir: '/Apps/Caramba.app/Contents/MacOS',
        workingDir: '/repo/caramba/apps/caramba-client',
      );
      expect(candidates.first, '/opt/core/libcaramba_core.dylib');
    });

    test('ignores a blank env override', () {
      final candidates = carambaCoreLibraryCandidates(
        envOverride: '   ',
        executableDir: '/Apps/Caramba.app/Contents/MacOS',
      );
      expect(
        candidates.first,
        '/Apps/Caramba.app/Contents/Frameworks/libcaramba_core.dylib',
      );
    });

    test('puts the bundle Frameworks path before the flat bundle path', () {
      final candidates = carambaCoreLibraryCandidates(
        executableDir: '/Apps/Caramba.app/Contents/MacOS',
      );
      expect(candidates, <String>[
        '/Apps/Caramba.app/Contents/Frameworks/libcaramba_core.dylib',
        '/Apps/Caramba.app/Contents/MacOS/libcaramba_core.dylib',
      ]);
    });

    test('walks up from the working dir to the repo dev build path', () {
      final candidates = carambaCoreLibraryCandidates(
        workingDir: '/repo/caramba/apps/caramba-client',
      );
      expect(
        candidates,
        contains('/repo/caramba/libs/caramba-core/build/libcaramba_core.dylib'),
      );
      // Каждый предок пробуется, включая сам рабочий каталог и корень.
      expect(
        candidates,
        contains(
          '/repo/caramba/apps/caramba-client/libs/caramba-core/build/'
          'libcaramba_core.dylib',
        ),
      );
      // Подъём останавливается на первом сегменте, до корня `/` не доходит.
      expect(
        candidates.last,
        '/repo/libs/caramba-core/build/libcaramba_core.dylib',
      );
    });

    test('also walks up from the script dir (flutter run / flutter test)', () {
      final candidates = carambaCoreLibraryCandidates(
        scriptDir: '/repo/caramba/apps/caramba-client/test',
        workingDir: '/somewhere/else',
      );
      expect(
        candidates,
        contains('/repo/caramba/libs/caramba-core/build/libcaramba_core.dylib'),
      );
    });

    test('deduplicates while preserving order', () {
      final candidates = carambaCoreLibraryCandidates(
        envOverride: '/repo/libs/caramba-core/build/libcaramba_core.dylib',
        workingDir: '/repo',
        scriptDir: '/repo',
      );
      const repoPath = '/repo/libs/caramba-core/build/libcaramba_core.dylib';
      expect(candidates.where((p) => p == repoPath), hasLength(1));
      expect(candidates.first, repoPath);
    });

    test('normalizes .. and duplicate separators', () {
      final candidates = carambaCoreLibraryCandidates(
        executableDir: '/Apps//Caramba.app/Contents/./MacOS',
      );
      expect(
        candidates.first,
        '/Apps/Caramba.app/Contents/Frameworks/libcaramba_core.dylib',
      );
    });

    test('uses the platform file name for linux and windows', () {
      expect(
        carambaCoreLibraryCandidates(
          executableDir: '/opt/app',
          isMacOS: false,
          isWindows: false,
        ),
        contains('/opt/app/libcaramba_core.so'),
      );
      expect(
        carambaCoreLibraryCandidates(
          executableDir: 'C:/app',
          isMacOS: false,
          isWindows: true,
        ),
        contains('C:/app/caramba_core.dll'),
      );
    });

    test('returns nothing when it has no hints at all', () {
      expect(carambaCoreLibraryCandidates(), isEmpty);
    });
  });
}

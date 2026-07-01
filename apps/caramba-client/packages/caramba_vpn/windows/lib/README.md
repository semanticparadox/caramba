# Windows native runtime libraries go here

This directory must contain the two Windows runtime DLLs. They are gitignored as
build artifacts; the plugin `CMakeLists.txt` bundles them next to
`caramba_client.exe` via `caramba_vpn_bundled_libraries`.

## libcaramba_core.dll

The Go engine built as a cgo c-shared library for `GOOS=windows`, exporting the
`Caramba*` C ABI from `../include/caramba_core.h`. Build on a machine with the Go
toolchain + mingw-w64, then vendor it here (see INTEGRATION.md step 0):

```bash
# from the repo root
cd libs/caramba-core
scripts/build-desktop-lib.sh    # -> libs/caramba-core/build/libcaramba_core.dll
cp build/libcaramba_core.dll \
   ../../apps/caramba-client/packages/caramba_vpn/windows/lib/
```

Build the Go side with the `mihomo` build tag so the real engine (not the stub)
is linked, so packets actually flow.

## wintun.dll

The user-mode TUN driver mihomo opens to create the tunnel adapter. Download the
signed release from https://www.wintun.net and copy the arch-matching DLL
(`wintun/bin/amd64/wintun.dll` or `arm64`) here.

Both DLLs must match the host build architecture. The app must run elevated
(`requestedExecutionLevel=requireAdministrator`) so wintun can create the
adapter — see INTEGRATION.md step 2 (Windows).

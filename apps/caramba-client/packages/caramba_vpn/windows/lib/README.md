# Windows native runtime libraries go here

This directory must contain the two Windows runtime DLLs. They are gitignored as
build artifacts (`packages/caramba_vpn/.gitignore`: `windows/lib/*.dll`); the
plugin `CMakeLists.txt` bundles them next to `caramba_client.exe` via
`caramba_vpn_bundled_libraries`. Both are produced by scripts, so the same two
commands work locally and in CI — nothing here is fetched by hand.

Both DLLs must match the host build architecture (`--arch amd64` by default;
`arm64` on both scripts for an ARM Windows build).

## libcaramba_core.dll

The Go engine built as a cgo c-shared library for `GOOS=windows`, exporting the
`Caramba*` C ABI from `../include/caramba_core.h`. The name carries the `lib`
prefix everywhere: the plugin loads it with
`LoadLibraryW(L"libcaramba_core.dll")` (`../caramba_core_ffi.h`), the dart:ffi
path looks up the same name (`lib/src/ffi/library_lookup.dart`), and the build
script writes exactly that.

```bash
# from the repo root — works on macOS/Linux (mingw-w64 cross) and on Windows
libs/caramba-core/scripts/build-windows-lib.sh
```

The script builds with the `mihomo,with_gvisor` tags against the patched mihomo
copy (`mk-patched-deps.sh`) — without that patch the TUN adapter does not start
— and copies the result here itself. Cross-compiling needs mingw-w64
(`brew install mingw-w64`, or `apt-get install gcc-mingw-w64`); on a Windows
runner the stock `gcc` is used.

## wintun.dll

The user-mode TUN driver mihomo opens to create the tunnel adapter. It is not
built from source (the shipped DLL is WHQL-signed), it is downloaded from
wintun.net and checked against a pinned SHA-256:

```bash
libs/caramba-core/scripts/fetch-wintun.sh
```

## Running

The app must run elevated so wintun can create the adapter.
`windows/runner/runner.exe.manifest` now declares
`requestedExecutionLevel=requireAdministrator`, so Windows shows a UAC prompt
on every launch (and the Inno Setup installer runs the post-install launch
elevated for the same reason); see `docs/WINDOWS.md`.

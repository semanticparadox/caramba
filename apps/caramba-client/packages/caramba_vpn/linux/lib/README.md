# Linux native runtime library goes here

This directory must contain `libcaramba_core.so`. It is gitignored as a build
artifact; the plugin `CMakeLists.txt` bundles it into the app bundle's `lib/`
next to the host binary via `caramba_vpn_bundled_libraries` (the bundle ships
with an rpath of `$ORIGIN/lib`, so the runtime `dlopen` resolves it).

## libcaramba_core.so

The Go engine built as a cgo c-shared library for the host arch, exporting the
`Caramba*` C ABI from `../include/caramba_core.h`. Build with the Go toolchain +
a system C compiler, then vendor it here (see INTEGRATION.md step 0):

```bash
# from the repo root
cd libs/caramba-core
scripts/build-desktop-lib.sh    # -> libs/caramba-core/build/libcaramba_core.so
cp build/libcaramba_core.so \
   ../../apps/caramba-client/packages/caramba_vpn/linux/lib/
```

Build the Go side with the `mihomo` build tag so the real engine (not the stub)
is linked, so packets actually flow. mihomo creates the tun device itself, so the
host binary needs root or `CAP_NET_ADMIN` (`setcap` or `pkexec`) — see
INTEGRATION.md step 2 (Linux).

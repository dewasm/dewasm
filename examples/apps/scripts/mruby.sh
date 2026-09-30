#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# mruby: built with `wasi-sdk` from the 3.4.0 release source, a fixed version.
# The build is mruby's own `rake` build (`src/mruby_build_config.rb`).
#
# This app exists to exercise the wasm exception-handling proposal.
# mruby's raise/rescue/ensure lower to C `setjmp`/`longjmp` (`include/mruby/throw.h`).
# wasm32 has no native `setjmp`/`longjmp`.
# So every object is compiled with LLVM's SJLJ lowering.
# The lowering rewrites them into `try_table`/`throw`.
# The lowering flags are `-mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false`.
# The link adds `-lsetjmp`, the prebuilt runtime of `wasi-sdk` for that lowering.
# `SetjmpLongjmp.md` in the `wasi-sdk` repository documents it.
# `wasm-opt` is never run on the result, unlike the other apps built with `wasi-sdk`.
# `wasm_opt_inplace` in `common.sh` is fixed to a baseline feature set.
# That set never includes exception-handling.
# This module exists specifically to carry EH instructions.
#
# Gem selection: the WASI build cannot include `mruby-io`, `mruby-dir`, or `mruby-socket`.
# `src/io.c` of `mruby-io` unconditionally `#include <sys/wait.h>` for IO.popen (fork+wait).
# `wasi-libc` ships no such header at all, and no define works around it.
# `mruby-socket` needs `<sys/socket.h>`/`<netinet/*.h>`.
# `wasi-libc` likewise never provides those (WASI preview1 has no BSD sockets).
# `mruby-dir` is an easier case: its only WASI blocker is <signal.h>.
# `wasi-libc` guards that header with an #error unless built with -D_WASI_EMULATED_SIGNAL.
# But `mruby-dir` stays out too: nothing in MRUBY_GEMS needs that emulation.
# Pulling in Dir for its own sake is out of scope for this fixture.
# MRUBY_GEMS below is the general-purpose standard library set confirmed to compile clean on WASI.
# `mruby-print` is not a gem: Kernel#print/#p are core (`src/print.c`), always built in.
# Losing `mruby-io` also loses Kernel#puts.
# `Kernel#puts` is `mruby-io/mrblib/kernel.rb` (`$stdout.puts`), not core.
# `src/mruby-wasi-puts` (listed in mruby_build_config.rb) restores it.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

MRUBY_URL="https://github.com/mruby/mruby/archive/refs/tags/3.4.0.tar.gz"
MRUBY_SHA256="183711c7a26d932b5342e64860d16953f1cc6518d07b2c30a02937fb362563f8"
MRUBY_DIR="mruby-3.4.0"

MRUBY_GEMS=(
  mruby-sprintf mruby-math mruby-string-ext mruby-array-ext mruby-enum-ext
  mruby-hash-ext mruby-numeric-ext mruby-symbol-ext mruby-object-ext
  mruby-error mruby-metaprog mruby-pack mruby-random mruby-time
)

# The stamp covers the source checksum, the gem list, and the toolchain token.
# So changing any retriggers the build.
mruby_key="$MRUBY_SHA256 gems:${MRUBY_GEMS[*]} $(wasi_sdk_stamp)"
mruby_stamp="cache/mruby.src-sha256"
if is_cached "$mruby_stamp" "$mruby_key" cache/mruby.wasm; then
  echo "mruby: cached"
  exit 0
fi

require_wasi_sdk mruby
# The SJLJ lowering's runtime hooks; shipped prebuilt since wasi-sdk-26.
# wasi-sdk-26 also added the non-legacy EH flag this build passes.
[ -f "$WASI_SDK_PATH/share/wasi-sysroot/lib/wasm32-wasip1/libsetjmp.a" ] || {
  echo "mruby: this wasi-sdk ships no libsetjmp for wasm32-wasip1; $WASI_SDK_HINT" >&2
  exit 1
}
require_tool mruby ruby "install a host Ruby (e.g. via a version manager, or brew install ruby) to run mruby's rake build"
require_tool mruby rake "install rake (e.g. \`gem install rake\`) to run mruby's build"

echo "mruby: fetching $MRUBY_URL"
new_tmpdir
fetch_verified "$MRUBY_URL" "$MRUBY_SHA256" "$tmp/mruby.tar.gz"
tar xzf "$tmp/mruby.tar.gz" -C "$tmp"

MRUBY_EH_FLAGS=(-mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false)

# Compile wrapper: every object needs the SJLJ flags.
# Link wrapper: `-lsetjmp` after the objects, plus --strip-debug.
# Otherwise mruby.wasm carries full DWARF, ~5x the stripped size.
# `wasm-opt`, which strips it for the other apps, cannot run here.
cc_wrapper="$tmp/mruby-cc.sh"
{
  echo '#!/bin/sh'
  printf 'exec "%s/bin/clang" --target=wasm32-wasip1 -O2' "$WASI_SDK_PATH"
  printf ' %s' "${MRUBY_EH_FLAGS[@]}"
  printf ' "$@"\n'
} >"$cc_wrapper"
chmod +x "$cc_wrapper"

# `--no-wasm-opt` for the same reason as `wasi_sdk_clang` in `common.sh`.
# It is doubly load-bearing here, where the module must keep its EH instructions.
ld_wrapper="$tmp/mruby-ld.sh"
printf '#!/bin/sh\nexec "%s/bin/clang" --target=wasm32-wasip1 --no-wasm-opt -Wl,--strip-debug "$@" -lsetjmp\n' "$WASI_SDK_PATH" >"$ld_wrapper"
chmod +x "$ld_wrapper"

ar_wrapper="$tmp/mruby-ar.sh"
printf '#!/bin/sh\nexec "%s/bin/llvm-ar" "$@"\n' "$WASI_SDK_PATH" >"$ar_wrapper"
chmod +x "$ar_wrapper"

echo "mruby: building mruby.wasm (rake, wasi-sdk clang, LLVM SJLJ lowering)"
build_config="$(pwd)/src/mruby_build_config.rb"
jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)
(
  cd "$tmp/$MRUBY_DIR"
  MRUBY_CONFIG="$build_config" \
    DEWASM_MRUBY_CC="$cc_wrapper" \
    DEWASM_MRUBY_LD="$ld_wrapper" \
    DEWASM_MRUBY_AR="$ar_wrapper" \
    DEWASM_MRUBY_GEMS="${MRUBY_GEMS[*]}" \
    rake -j"$jobs"
)
cp "$tmp/$MRUBY_DIR/build/wasm32-wasi/bin/mruby.wasm" cache/mruby.wasm

write_stamp "$mruby_stamp" "$mruby_key"
echo "mruby: -> cache/mruby.wasm"

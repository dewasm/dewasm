#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# mruby: built from the pinned 3.4.0 release source with zig, through
# mruby's own `rake` build (src/mruby_build_config.rb).
#
# This app exists to exercise the wasm exception-handling proposal: mruby's
# raise/rescue/ensure lower to C setjmp/longjmp (include/mruby/throw.h), and
# wasm32 has no native setjmp/longjmp, so every object is compiled with
# LLVM's SJLJ lowering (`-mexception-handling -mllvm -wasm-enable-sjlj
# -mllvm -wasm-use-legacy-eh=false`), which rewrites them into
# `try_table`/`throw`. wasm-opt is never run on the result (unlike the
# sibling zig-built apps): common.sh's wasm_opt_inplace is pinned to a
# baseline feature set that never includes exception-handling, and this
# module exists specifically to carry EH instructions.
#
# zig 0.16 trap: passing -mexception-handling at LINK time makes zig build
# wasi-libc's own setjmp runtime (rt.c) on demand WITHOUT the -mllvm flags
# above, and it dies ("undefined tag symbol cannot be weak"). Workaround:
# compile rt.c ourselves with the full flag set and feed the resulting rt.o
# straight into the link, which then runs with no EH flags at all (so zig
# never attempts that on-demand rebuild). rt.c ships inside zig's own libc
# sysroot, so its path is derived from `zig env`, not hardcoded.
#
# Gem selection: the wasi build cannot include mruby-io, mruby-dir, or
# mruby-socket. mruby-io's src/io.c unconditionally `#include <sys/wait.h>`
# for IO.popen (fork+wait); wasi-libc ships no such header at all, no define
# works around it. mruby-socket needs <sys/socket.h>/<netinet/*.h>, which
# wasi-libc likewise never provides (WASI preview1 has no BSD sockets).
# mruby-dir is milder (its only wasi blocker is <signal.h>, which wasi-libc
# poisons behind an #error unless built with -D_WASI_EMULATED_SIGNAL) but
# stays out too: nothing in MRUBY_GEMS needs that emulation, and pulling in
# Dir for its own sake is out of scope for this fixture.
# MRUBY_GEMS below is the general-purpose stdlib set confirmed to compile
# clean on wasi; mruby-print does not exist as a gem (Kernel#print/#p are
# core, src/print.c, always built in). Losing mruby-io also loses
# Kernel#puts (it is mruby-io/mrblib/kernel.rb: `$stdout.puts`, not core);
# src/mruby-wasi-puts (wired in mruby_build_config.rb) restores it.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

MRUBY_URL="https://github.com/mruby/mruby/archive/refs/tags/3.4.0.tar.gz"
MRUBY_SHA256="183711c7a26d932b5342e64860d16953f1cc6518d07b2c30a02937fb362563f8"
MRUBY_DIR="mruby-3.4.0"

MRUBY_GEMS=(
  mruby-sprintf mruby-math mruby-string-ext mruby-array-ext mruby-enum-ext
  mruby-hash-ext mruby-numeric-ext mruby-symbol-ext mruby-object-ext
  mruby-error mruby-metaprog mruby-pack mruby-random mruby-time
)

# The stamp covers the source sha and the gem list, so bumping either retriggers the build.
mruby_key="$MRUBY_SHA256 gems:${MRUBY_GEMS[*]}"
mruby_stamp="cache/mruby.src-sha256"
if is_cached "$mruby_stamp" "$mruby_key" cache/mruby.wasm; then
  echo "mruby: cached"
  exit 0
fi

require_tool mruby zig "install zig (e.g. brew install zig) to build the mruby app"
require_tool mruby ruby "install a host Ruby (e.g. via a version manager, or brew install ruby) to run mruby's rake build"
require_tool mruby rake "install rake (e.g. \`gem install rake\`) to run mruby's build"

echo "mruby: fetching $MRUBY_URL"
new_tmpdir
fetch_verified "$MRUBY_URL" "$MRUBY_SHA256" "$tmp/mruby.tar.gz"
tar xzf "$tmp/mruby.tar.gz" -C "$tmp"

MRUBY_EH_FLAGS=(-mexception-handling -mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false)

zig_lib_dir=$(zig env | sed -n 's/.*\.lib_dir = "\(.*\)",/\1/p')
rt_c="$zig_lib_dir/libc/wasi/libc-top-half/musl/src/setjmp/wasm32/rt.c"
[ -f "$rt_c" ] || {
  echo "mruby: zig's wasi setjmp runtime not found at $rt_c (zig layout changed?)" >&2
  exit 1
}
echo "mruby: building rt.o (zig's wasi setjmp runtime, with SJLJ lowering)"
zig_cc_wasi "${MRUBY_EH_FLAGS[@]}" -O2 -c -o "$tmp/rt.o" "$rt_c"

# Compile wrapper: every object needs the SJLJ flags. Link wrapper: NONE of
# them (the zig-0.16 trap above), plus rt.o and --strip-debug (mruby.wasm
# carries full DWARF otherwise, ~5x the stripped size; wasm-opt, which
# strips it for the sibling apps, cannot run here).
cc_wrapper="$tmp/mruby-cc.sh"
{
  echo '#!/bin/sh'
  printf 'exec zig cc -target wasm32-wasi -O2'
  printf ' %s' "${MRUBY_EH_FLAGS[@]}"
  printf ' "$@"\n'
} >"$cc_wrapper"
chmod +x "$cc_wrapper"

ld_wrapper="$tmp/mruby-ld.sh"
printf '#!/bin/sh\nexec zig cc -target wasm32-wasi -Wl,--strip-debug "$@" %s\n' "$tmp/rt.o" >"$ld_wrapper"
chmod +x "$ld_wrapper"

ar_wrapper="$tmp/mruby-ar.sh"
printf '#!/bin/sh\nexec zig ar "$@"\n' >"$ar_wrapper"
chmod +x "$ar_wrapper"

echo "mruby: building mruby.wasm (rake, zig cc, LLVM SJLJ lowering)"
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

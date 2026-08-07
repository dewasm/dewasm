#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# libpcap: BPF filter compiler, built from the pinned upstream source release
# with zig as a reactor library. Only the platform-independent
# filter-compilation TUs are built (no capture backend); src/pcap_config.h
# stands in for ./configure's config.h (see its header comment), and our own
# src/pcap_binding.c exports compile_filter(), which turns a textual filter
# like "tcp port 80" into a serialized BPF program in guest memory. libpcap
# 1.10.x no longer ships pre-generated grammar.c/scanner.c, so the parser is
# regenerated here with bison + flex (matching the substitution ./configure
# would apply for a bison >= 3 reentrant parser).

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

PCAP_URL="https://www.tcpdump.org/release/libpcap-1.10.6.tar.gz"
PCAP_SHA256="872dd11337fe1ab02ad9d4fee047c9da244d695c6ddf34e2ebb733efd4ed8aa9"
PCAP_DIR="libpcap-1.10.6"
# The filter-compiler TUs plus the libpcap internals the linker demands
# (fmtutils/etherent/strlcpy) and the generated parser (grammar/scanner).
PCAP_SRCS=(
  pcap.c gencode.c optimize.c nametoaddr.c bpf_image.c bpf_filter.c
  grammar.c scanner.c pcap-common.c fmtutils.c etherent.c missing/strlcpy.c
)

pcap_stamp="cache/libpcap.src-sha256"
pcap_want="$(printf '%s\n%s' "$PCAP_SHA256" "$(wasm_opt_version)")"
if is_cached "$pcap_stamp" "$pcap_want" cache/libpcap.wasm; then
  echo "libpcap: cached"
  exit 0
fi

require_tool libpcap zig "install zig (e.g. brew install zig) to build the libpcap app"
require_tool libpcap wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess the libpcap app"
require_tool libpcap bison "install bison (e.g. brew install bison) to regenerate the filter grammar"
require_tool libpcap flex "install flex (e.g. brew install flex) to regenerate the filter scanner"

echo "libpcap: fetching $PCAP_URL"
new_tmpdir
fetch_verified "$PCAP_URL" "$PCAP_SHA256" "$tmp/libpcap.tar.gz"
tar xzf "$tmp/libpcap.tar.gz" -C "$tmp"
pdir="$tmp/$PCAP_DIR"
echo "libpcap: generating grammar.c/scanner.c (bison/flex)"
sed 's/@REENTRANT_PARSER@/%define api.pure/' "$pdir/grammar.y.in" >"$pdir/grammar.y"
( cd "$pdir" && bison -p pcap_ -o grammar.c -d grammar.y \
  && flex -P pcap_ --header-file=scanner.h --nounput -o scanner.c scanner.l )
cp src/pcap_config.h "$pdir/config.h"
echo "libpcap: building libpcap.wasm (zig cc, reactor)"
psrcs=()
for s in "${PCAP_SRCS[@]}"; do psrcs+=("$pdir/$s"); done

# pcap_compile_nopcap() is the documented filter-only entry point but is
# marked deprecated (thread-safety of its error buffer); silence that here.
# --strip-debug drops the DWARF wasm-opt cannot process.
zig_cc_wasi -mexec-model=reactor -O2 \
  -DBUILDING_PCAP -D_WASI_EMULATED_SIGNAL -lwasi-emulated-signal \
  -Wno-deprecated-declarations -Wl,--strip-debug -I "$pdir" \
  "${psrcs[@]}" src/pcap_binding.c \
  -Wl,--export=compile_filter -Wl,--export=malloc -Wl,--export=free \
  -o cache/libpcap.wasm
echo "libpcap: wasm-opt -O2"
wasm_opt_inplace cache/libpcap.wasm

write_stamp "$pcap_stamp" "$pcap_want"
echo "libpcap: -> cache/libpcap.wasm"

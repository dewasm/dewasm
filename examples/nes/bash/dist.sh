#!/usr/bin/env bash
# Build nes.bash: the single-file distributable — the terminal frontend with
# the generated library inlined in place of its `source` line and the demo
# ROM base64-embedded, behind a provenance header. One file that runs
# anywhere with bash >= 5, no dewasm checkout needed.
#
# Unlike DOOM's dist.sh, there is no licensing reason to keep this out of the
# repository: agnes (the emulator, kgabis/agnes) is MIT, nes_demo.c is our own
# MIT source, and the embedded ROM (Alter Ego by Shiru) is public domain. It
# is still built rather than committed only because it embeds a copy of the
# generated library (regenerated on every build) and the ROM.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh >&2

rom=../../apps/cache/alter_ego.nes
if [[ ! -r $rom ]]; then
  echo "nes (bash): ROM not found at $rom (run ../../apps/scripts/nes.sh)" >&2
  exit 1
fi
rom_b64=$(base64 < "$rom" | tr -d '\n')

out=nes.bash
{
  echo '#!/usr/bin/env bash'
  cat <<'HDR'
#
# nes.bash — an NES emulator running in pure GNU Bash. No compiled code, no
# dependencies.
#
# This one file contains three things:
#   1. The agnes NES emulator (https://github.com/kgabis/agnes, MIT) wrapped by
#      our own nes_demo.c, compiled to a WebAssembly module and then translated
#      from WebAssembly into Bash source by dewasm
#      (https://github.com/dewasm/dewasm — the same wasm binary also runs there
#      as pure Go, Java, Ruby, Python, and Perl; see examples/nes).
#   2. The demo ROM, base64-embedded: Alter Ego by Shiru, released into the
#      public domain (https://shiru.untergrund.net).
#   3. A terminal frontend: ANSI truecolor half-block rendering, raw-mode input.
#
# Run:      bash nes.bash            (needs bash >= 5 and a truecolor terminal)
# Or:       bash nes.bash rom.nes    (your own iNES ROM instead of Alter Ego)
# Or:       bash nes.bash --smoke    (headless self-check, writes screenshot.ppm)
#
# Honest expectations: bash interprets a full NES frame's emulation per tick
# with no JIT, so this is an existence proof, not a game you can play at speed.
# Press q to quit; the terminal is restored on exit. Controls: arrows = D-pad,
# x = A, z = B, Enter = Start, Space = Select.
#
# License: nes_demo.c and dewasm are MIT; agnes is MIT; the embedded ROM is
# public domain. This is a generated file: regenerate it with
# examples/nes/bash/dist.sh in the dewasm repository.
#
HDR
  printf 'EMBEDDED_ROM_B64=%q\n' "$rom_b64"
  awk '
    NR == 1 && /^#!/ { next }
    /^source \.\/nes_gen\.sh$/ {
      while ((getline line < "nes_gen.sh") > 0) print line
      close("nes_gen.sh")
      next
    }
    { print }
  ' main.sh
} > "$out"
chmod +x "$out"

bash -n "$out"
echo "built $out ($(du -h "$out" | cut -f1 | tr -d ' '))"

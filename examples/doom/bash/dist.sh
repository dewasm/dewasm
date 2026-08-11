#!/usr/bin/env bash
# Build doom.bash: the single-file distributable, the terminal frontend with the generated library inlined in place of its `source` line, behind a provenance/license header. The result embeds the GPL-2.0 DOOM engine and the shareware WAD, so it is published as a standalone artifact (a Gist linked from the READMEs) rather than committed to this repository.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh >&2

out=doom.bash
{
  echo '#!/usr/bin/env bash'
  cat <<'HDR'
#
# doom.bash: DOOM, running in pure GNU Bash. No compiled code, no dependencies.
#
# This one file contains two things:
#   1. The 1993 DOOM engine (via doomgeneric), compiled to a WebAssembly module
#      by jacobenget/doom.wasm v0.1.0 with the DOOM shareware WAD embedded, then
#      translated from WebAssembly into Bash source by dewasm
#      (https://github.com/dewasm/dewasm, the same wasm binary also runs there
#      as pure Go, Java, Ruby, and Python; see examples/doom).
#   2. A terminal frontend: ANSI truecolor half-block rendering, raw-mode input.
#
# Run:      bash doom.bash          (needs bash >= 5 and a truecolor terminal)
# Or:       bash doom.bash --smoke  (headless self-check, writes screenshot.ppm)
#
# Honest expectations, measured on an Apple Silicon laptop: ~25s to
# instantiate, ~100s for DOOM's own init, then about one frame every ~34
# seconds. An existence proof, not a game you can play. Press q to quit;
# the terminal is restored on exit.
#
# License: GNU GPL v2. The embedded engine is doomgeneric
# (https://github.com/ozkl/doomgeneric, GPL-2.0), wasm build by
# https://github.com/jacobenget/doom.wasm (GPL-2.0). The embedded shareware
# WAD is id Software's freely-distributable shareware data. DOOM is a
# trademark of id Software. This is a generated file: regenerate it with
# examples/doom/bash/dist.sh in the dewasm repository.
#
HDR
  awk '
    NR == 1 && /^#!/ { next }
    /^source \.\/doom_gen\.sh$/ {
      while ((getline line < "doom_gen.sh") > 0) print line
      close("doom_gen.sh")
      next
    }
    { print }
  ' main.sh
} > "$out"
chmod +x "$out"

bash -n "$out"
echo "built $out ($(du -h "$out" | cut -f1 | tr -d ' '))"

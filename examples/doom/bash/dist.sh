#!/usr/bin/env bash
# Build doom.bash, the single-file distributable.
# It is the terminal frontend with the generated library inlined in place of its `source` line.
# A header stating its origin and license comes first.
# The result embeds the GPL-2.0 DOOM engine and the shareware WAD.
# So it is published as a standalone artifact (a Gist linked from the READMEs).
# It is not committed to this repository.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh >&2

out=doom.bash
{
  echo '#!/usr/bin/env bash'
  cat <<'HDR'
#
# `doom.bash`: DOOM, running in GNU Bash alone.
# It needs no compiled code and no dependencies.
#
# This one file holds two things:
#   1. The 1993 DOOM engine (`doomgeneric`), compiled to a WebAssembly module.
#      `jacobenget/doom.wasm` v0.1.0 built it, with the DOOM shareware WAD embedded.
#      dewasm then converted that module into Bash source (`https://github.com/dewasm/dewasm`).
#      The same wasm binary also runs there as Go, Java, Ruby, or Python alone; see `examples/doom`.
#   2. A terminal frontend: half-block rendering in 24-bit ANSI color, and raw-mode input.
#
# Run:      `bash doom.bash`          (needs Bash 5 or later and a 24-bit color terminal)
# Or:       `bash doom.bash --smoke`  (a headless self-check that writes `screenshot.ppm`)
#
# Measured on an Apple Silicon laptop:
# ~25s to instantiate, ~100s for DOOM's own initialization, then one frame every ~34 seconds.
# It is a proof that this runs, not a game you can play.
# Press q to quit; the terminal is restored on exit.
#
# License: GNU GPL v2.
# The embedded engine is `doomgeneric` (`https://github.com/ozkl/doomgeneric`, GPL-2.0).
# The wasm build is by `https://github.com/jacobenget/doom.wasm` (GPL-2.0).
# The embedded shareware WAD is id Software's shareware data, free to distribute.
# DOOM is a trademark of id Software.
# This is a generated file: `examples/doom/bash/dist.sh` in the dewasm repository makes it.
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

# NES on dewasm

One NES, six languages: [agnes](https://github.com/kgabis/agnes) (a dependency-free C NES emulation library, MIT) plus a thin wrapper ([`../apps/src/nes_demo.c`](../apps/src/nes_demo.c)) compiled to a single 19KB wasm module with an **empty import section**, converted by `dewasm --mode library` and played through a native frontend per language:

- [`go/`](go/) — Go, rendering with [ebiten](https://github.com/hajimehoshi/ebiten)
- [`java/`](java/) — Java, rendering with Swing (plain JDK, zero dependencies)
- [`ruby/`](ruby/) — Ruby, rendering *into the terminal* as 24-bit-color ANSI half-blocks (stdlib only, run with `--yjit`)
- [`python/`](python/) — Python, the same terminal renderer (stdlib only, ~1.2 frames/sec)
- [`perl/`](perl/) — Perl, the same terminal renderer (core modules only, ~0.9 frames/sec)
- [`bash/`](bash/) — pure Bash, same terminal renderer; ~25–50 seconds per frame, an existence proof in the bash-DOOM tradition (with a `dist.sh` that builds a single-file `nes.bash` — everything here is MIT/public-domain, so unlike DOOM's GPL build it needs no out-of-repo home)

Where DOOM demonstrates library mode's *import* surface (ten host functions), the NES module needs nothing from the host at all: a NES frame is a fixed unit of console time, so pacing, input polling, and presentation are wholly host-side. Each frontend reads a `.nes` ROM file, feeds it in through the exported `allocRom`, then drives `initGame`/`setInput`/`tickGame` and reads the framebuffer (256×240, B,G,R,A) straight out of exported memory. The wasm module is the portable artifact and — unlike DOOM, whose WAD is baked in — the program it runs is your choice: pass any ROM within agnes's mapper coverage (NROM/UxROM/MMC1/MMC3) as an argument ([ADR-59](../../docs/adr/59-nes-example-agnes.md)).

![The deterministic NES frame snapshot](../apps/snapshots/nes_frame.png)

*The frame the framebuffer-snapshot test pins: 40 input-free frames into [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) (the bundled demo ROM — a puzzle platformer by Shiru, public domain), every backend and the wasmtime oracle render these exact pixels, so it doubles as a cross-backend conformance snapshot in the DOOM snapshot's harness ([ADR-53](../../docs/adr/53-doom-frame-golden.md)). The compared oracle is `nes_frame.ppm`; this PNG is the same frame for human eyes.*

## Run

```sh
go/run.sh    # or: java/run.sh, ruby/run.sh, ...
go/run.sh path/to/other.nes   # any ROM agnes's mappers cover
```

`build.sh` fetches agnes and the Alter Ego ROM (checksum-pinned) and compiles `nes.wasm` with `zig cc` (via `../apps/scripts/nes.sh`) into the gitignored apps cache. Each frontend also has a headless `-smoke`/`--smoke` mode that ticks the emulator without a window/tty, sanity-checks the rendered frame, and writes it to a screenshot file.

Measured on an Apple Silicon laptop, headless: Go ~228 ticks/sec, Java ~126 — both far above the NES's ~60Hz frame rate, so the windowed frontends pace themselves down to 60. Ruby reaches ~10.5 with YJIT, Python ~1.2, Perl ~0.9, which is why those three render into the terminal instead of a window. Bash draws a frame every ~25–50 seconds — not playable, but genuinely emulating. Terminals report key presses but not releases, so the terminal frontends synthesize key-up events after a short hold window.

Controls (all frontends): arrows = D-pad, `x` = A, `z` = B, Enter = Start, Space = Select, `q`/Esc = quit.

No sound: agnes has no APU. The frontends are built by their own scripts and are not part of `cargo test`; the frame snapshot above is, at the DOOM case's tiers (slow, Bash at ultra).

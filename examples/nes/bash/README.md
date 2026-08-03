# NES (Bash, ANSI terminal)

An NES emulator running in pure GNU Bash. `build.sh` builds the checksum-pinned [agnes](https://github.com/kgabis/agnes) emulator (wrapped by our own [`nes_demo.c`](../../apps/src/nes_demo.c)) into a single import-free wasm module via [`../../apps/scripts/nes.sh`](../../apps/scripts/nes.sh), then runs it through `dewasm --target bash --mode library` to produce `nes_gen.sh` (generated Bash, gitignored, regenerated on every build), and `main.sh` loads a ROM into the module's memory, drives the exported `initGame`/`setInput`/`tickGame`, and renders the framebuffer into any ANSI truecolor terminal with half-block characters — the same shape as the DOOM Bash frontend ([`../../doom/bash`](../../doom/bash)) and the other NES frontends ([ADR-50](../../../docs/adr/50-doom-example-shape.md)). The NES CPU + PPU emulation is entirely the generated Bash; nothing about the emulator is reimplemented here.

The demo ROM is **Alter Ego by Shiru**, released into the [public domain](https://shiru.untergrund.net). Pass a path to `run.sh`/`main.sh` to play a different iNES ROM.

## Honest performance

This is an existence-proof demo, not a playable emulator, and the numbers say so plainly: **each `tickGame` (one 60Hz NES video frame) takes roughly 25-50 seconds** (measured on an Apple Silicon laptop, headless). A real NES runs 60 of those a second; this frontend delivers one every half-minute or so, on the order of ~1000-2000x slower than real time. Rendering is not the bottleneck: sampling and drawing one frame into the terminal costs about 0.2s. The wasm execution — Bash interpreting the 6502 CPU core and the PPU scanline renderer, instruction by instruction, with no JIT — is the entire cost.

| Phase | Time (reference machine) |
| --- | --- |
| Source `nes_gen.sh` + `nes_init` (memory + data/elem segments) | well under 1s |
| Load ROM (`allocRom` + byte copy into memory + `initGame`) | ~1.3s |
| `tickGame` (each — one emulated video frame) | ~25s early, rising to ~50s |
| Render one frame into the terminal | ~0.2s |

Frame cost is not flat: the 40-frame boot-to-credits self-check measured ~25-27s per frame for the first ~19 frames (near-black boot, little to draw), then ~48-50s per frame once Alter Ego's credits screen is up and animating — the busier the PPU output, the more per-frame emulation Bash does — for a mean of ~38s/frame over the run. Sustained-100%-CPU thermal throttling on a laptop likely contributes to the later figures too; treat the ~25s early number as the cleaner floor and ~50s as a loaded, busy-screen ceiling.

NES's 256×240 framebuffer is about 4.5x fewer pixels than DOOM's 640×400, and indeed a frame ticks in ~25s here versus DOOM's ~34s — but the two aren't directly comparable (different guest workload entirely; DOOM also pays a much heavier `initGame`). The point is the same: genuinely running, not fast.

Alter Ego opens on a near-black boot frame and only fades in its final, stable credits screen (7 distinct colors) by frame ~37, so the first ~15 frames of any run are essentially black while the ROM warms up the PPU — that is the ROM's own behavior, not a stall. The cross-backend framebuffer snapshot ([ADR-53](../../../docs/adr/53-doom-frame-golden.md)) pins frame 40 for exactly this reason.

## Run

```sh
./run.sh                 # default: Alter Ego
./run.sh path/to/rom.nes # a different iNES ROM
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input); expect roughly one rendered frame every 25-50 seconds. `./run.sh --smoke` instead runs a headless self-check: it loads the ROM, ticks 40 frames with no input (the same count as the framebuffer snapshot, so the result is Alter Ego's recognizable credits screen rather than the black boot frame), renders that frame, sanity-checks it (asserting the ~7-color credits screen actually drew), writes it to `screenshot.ppm` (ASCII PPM, P3 — plain text, so a stray NUL can't corrupt it, and Bash has no clean binary-safe way to write P6 anyway), and exits non-zero on failure. The full self-check measured about 25 minutes end to end (mean ~38s/frame); it prints a progress line before every tick specifically so a stretch of silence never looks like a hang. Set `SMOKE_FRAMES=N ./main.sh --smoke` for a quicker pipeline check (fewer than ~37 frames will legitimately render near-black and so fail the color assertion — the pipeline still exercises).

## Single-file distribution

`./dist.sh` builds `nes.bash`: the frontend with the generated library inlined in place of its `source` line and the demo ROM base64-embedded, behind a provenance header — one script that runs anywhere with bash >= 5, no dewasm checkout needed. Unlike the DOOM equivalent there is no licensing reason to keep it out of the repository (agnes is MIT, `nes_demo.c` is our own MIT source, the ROM is public domain); it is built rather than committed only because it embeds a fresh copy of the generated library and the ROM.

## Rendering

Same half-block trick as the DOOM frontends: each terminal cell shows two vertically-stacked source pixels as `▀`, colored with 24-bit truecolor SGR (`\e[38;2;R;G;Bm` for the top pixel, `\e[48;2;R;G;Bm` for the bottom). The 256×240 framebuffer lives in `nes_mem`, the module's linear memory — a plain Bash associative array, one byte per address (`B,G,R,A` per pixel, matching `doom.wasm`'s layout), read directly rather than copied out through a runtime call.

The sampled grid is capped at 128 columns (the NES's 256 native columns halved), which is already wider than most terminals people run this in. The render loop is written the same way DOOM's is even though it is nowhere near the bottleneck here: no per-cell `printf`, no command substitution in the hot path, the whole frame built as one string and emitted with a single `printf`, and an SGR escape skipped whenever it repeats the previous *cell's* color — nearly free, and it shrinks the string a lot on the NES's large flat-color areas (the PPU draws from a fixed 64-entry hardware palette, at most 25 colors on screen at once).

## Loading the ROM

Unlike DOOM, whose WAD is embedded in its wasm module and delivered through a host import, the NES module has **zero wasm imports**: the frontend allocates a buffer with `allocRom(size)`, copies the iNES ROM bytes straight into the module's linear memory at the returned guest pointer (`od -An -v -tu1` decodes the file to one byte per token; a ~42KB ROM is a few seconds of array assignments, once, at startup), then calls `initGame()`, which hands that buffer to agnes and returns 1 on success. `frameOffset`/`frameWidth`/`frameHeight` then report where the `256×240` framebuffer lives.

## Controls

Terminals deliver key *presses* only, never releases, and there is no meaningful "held key" at this frame rate — so whatever keys were pressed since the last tick are folded into a single controller bitmask, `setInput` is called with it just before `tickGame`, and the next frame starts from an empty set again (an unpressed key becomes `setInput(0)` on the following frame). One tick of "held down," as fine-grained as input can get at ~25s/frame.

| Key | NES button (bit) |
| --- | --- |
| Arrow keys | D-pad (Up 16, Down 32, Left 64, Right 128) |
| x | A (1) |
| z | B (2) |
| Enter | Start (8) |
| Space | Select (4) |
| q / Ctrl-C | Quit (terminal restored on exit) |

The button bits match [`nes_demo.c`](../../apps/src/nes_demo.c)'s `setInput`. There is no sound: the module exposes no audio interface.

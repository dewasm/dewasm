# NES (Bash, ANSI terminal)

An NES emulator running in pure GNU Bash.
`build.sh` builds the checksum-pinned [agnes](https://github.com/kgabis/agnes) emulator (wrapped by our own [`nes_demo.c`](../../apps/src/nes_demo.c)) into a single import-free wasm module via [`../../apps/scripts/nes.sh`](../../apps/scripts/nes.sh), then runs it through `dewasm --target bash --mode library` to produce `nes_gen.sh` (generated Bash, gitignored, regenerated on every build), and `main.sh` loads a ROM into the module's memory, drives the exported `initGame`/`setInput`/`tickGame`, and renders the framebuffer into any ANSI truecolor terminal with half-block characters, the same shape as the DOOM Bash frontend ([`../../doom/bash`](../../doom/bash)) and the other NES frontends.
The NES CPU + PPU emulation is entirely the generated Bash; nothing about the emulator is reimplemented here.

The demo ROM is **Alter Ego by Shiru**, released into the [public domain](https://shiru.untergrund.net).
Pass a path to `run.sh`/`main.sh` to play a different iNES ROM.

## Honest performance

This is an existence-proof demo, not a playable emulator: each `tickGame` (one NES video frame) takes tens of seconds, from ~17s for early boot frames to ~30s average across the full 40-frame boot-to-credits run (rendering itself is well under 1s; measured on an Apple Silicon laptop, headless).
Handing the frame over as agnes's own palette indices instead of a guest-rendered image (issue #117) cut that run from ~25 to ~20 minutes.

## Run

```sh
./run.sh                 # default: Alter Ego
./run.sh path/to/rom.nes # a different iNES ROM
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input); expect roughly one rendered frame every 20-40 seconds.
`./run.sh --smoke` instead runs a headless self-check: it loads the ROM, ticks 40 frames with no input (the same count as the framebuffer snapshot, so the result is Alter Ego's recognizable credits screen rather than the black boot frame), renders that frame, sanity-checks it (asserting the ~7-color credits screen actually drew), writes it to `screenshot.ppm` (ASCII PPM, P3: plain text, so a stray NUL can't corrupt it, and Bash has no clean binary-safe way to write P6 anyway), and exits non-zero on failure.
The full self-check measured about 20 minutes end to end (mean ~30s/frame); it prints a progress line before every tick specifically so a stretch of silence never looks like a hang.
Set `SMOKE_FRAMES=N ./main.sh --smoke` for a quicker pipeline check (fewer than ~37 frames will legitimately render near-black and so fail the color assertion: the pipeline still exercises).

## Rendering

Same half-block trick as the DOOM frontends: each terminal cell shows two vertically-stacked source pixels as `▀`, colored with 24-bit truecolor SGR (`\e[38;2;R;G;Bm` for the top pixel, `\e[48;2;R;G;Bm` for the bottom).
The 256×240 frame lives in `nes_mem`, the module's linear memory (a plain Bash associative array, one byte per address), and it is the emulator's own representation: one palette *index* per pixel (masked with `0x3f`), read directly rather than copied out through a runtime call.
The module's fixed 64-entry palette is read once at startup into ready-made SGR escape strings, so a sampled pixel costs one `nes_mem` read and one array lookup.

The sampled grid is capped at 128 columns (the NES's 256 native columns halved), which is already wider than most terminals people run this in.
The render loop is written the same way DOOM's is even though it is nowhere near the bottleneck here: no per-cell `printf`, no command substitution in the hot path, the whole frame built as one string and emitted with a single `printf`, and an SGR escape skipped whenever it repeats the previous *cell's* color.
That is nearly free, and it shrinks the string a lot on the NES's large flat-color areas (the PPU draws from a fixed 64-entry hardware palette, at most 25 colors on screen at once).

## Loading the ROM

Unlike DOOM's WAD (delivered through a host import), the NES module has zero wasm imports: the frontend allocates a buffer with `allocRom(size)`, copies the ROM bytes into it directly (a few seconds of array assignments for a ~42KB ROM, once, at startup), then calls `initGame()`, which hands the buffer to agnes.

## Controls

Terminals deliver key *presses* only, never releases, and there is no meaningful "held key" at this frame rate, so whatever keys were pressed since the last tick are folded into a single controller bitmask, `setInput` is called with it just before `tickGame`, and the next frame starts from an empty set again (an unpressed key becomes `setInput(0)` on the following frame).
One tick of "held down," as fine-grained as input can get at tens of seconds per frame.

| Key | NES button (bit) |
| --- | --- |
| Arrow keys | D-pad (Up 16, Down 32, Left 64, Right 128) |
| x | A (1) |
| z | B (2) |
| Enter | Start (8) |
| Space | Select (4) |
| q / Ctrl-C | Quit (terminal restored on exit) |

The button bits match [`nes_demo.c`](../../apps/src/nes_demo.c)'s `setInput`.
There is no sound: the module exposes no audio interface.

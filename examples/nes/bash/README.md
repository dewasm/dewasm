# NES (Bash, ANSI terminal)

An NES emulator running in pure GNU Bash.
`build.sh` builds the [`agnes`](https://github.com/kgabis/agnes) emulator into a single import-free wasm module.
It checks the emulator source against a fixed checksum.
Our own [`nes_demo.c`](../../apps/src/nes_demo.c) wraps the emulator.
The build goes via [`../../apps/scripts/nes.sh`](../../apps/scripts/nes.sh).
`dewasm --target bash --mode library` then converts the module to `nes_gen.sh`.
That file is generated Bash, ignored by Git, and regenerated on every build.
`main.sh` loads a ROM into the module's memory and drives the exported `initGame`/`setInput`/`tickGame`.
It renders the framebuffer as half-block characters into any terminal with 24-bit ANSI color.
This is the same shape as the DOOM Bash frontend ([`../../doom/bash`](../../doom/bash)) and the other NES frontends.
The NES CPU + PPU emulator is entirely the generated Bash.
Nothing about the emulator is reimplemented here.

The example ROM is **Alter Ego by Shiru**, released into the [public domain](https://shiru.untergrund.net).
Pass a path to `run.sh`/`main.sh` to play a different iNES ROM.

## Honest performance

This is an existence-proof example, not a playable emulator.
Each `tickGame` (one NES video frame) takes tens of seconds.
Early boot frames take ~17s, and the full 40-frame boot-to-credits run averages ~30s.
Rendering itself is well under 1s.
These were measured on an Apple Silicon laptop, headless.
Issue #117 changed the hand-off to the palette indices of `agnes` instead of a guest-rendered image.
That cut the run from ~25 to ~20 minutes.

## Run

```sh
./run.sh                 # default: Alter Ego
./run.sh path/to/rom.nes # a different iNES ROM
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input).
Expect roughly one rendered frame every 20-40 seconds.
`./run.sh --smoke` instead runs a headless self-check.
It loads the ROM and ticks 40 frames with no input.
That is the same count as the framebuffer snapshot.
So the result is Alter Ego's recognizable credits screen rather than the black boot frame.
It renders that frame and sanity-checks it, asserting the ~7-color credits screen actually drew.
It writes the frame to `screenshot.ppm` and exits non-zero on failure.
The file is ASCII PPM (P3), plain text, so an unexpected NUL byte can't damage it.
Bash has no clean binary-safe way to write P6 anyway.
The full self-check measured about 20 minutes end to end (mean ~30s/frame).
It prints a progress line before every tick so that a stretch of silence never looks like a hang.
Set `SMOKE_FRAMES=N ./main.sh --smoke` for a quicker check of every step.
Fewer than ~37 frames will legitimately render near-black and so fail the color assertion.
Every step still runs.

## Rendering

Same half-block trick as the DOOM frontends.
Each terminal cell shows two vertically-stacked source pixels as `▀`.
The cell is colored with 24-bit color SGR.
`\e[38;2;R;G;Bm` sets the top pixel, and `\e[48;2;R;G;Bm` the bottom.
The 256×240 frame lives in `nes_mem`, the module's linear memory.
That memory is a plain Bash associative array, one byte per address.
The frame is the emulator's own representation: one palette *index* per pixel (masked with `0x3f`).
It is read directly rather than copied out through a runtime call.
The module's fixed 64-entry palette is read once at start into ready-made SGR escape strings.
So a sampled pixel costs one `nes_mem` read and one array lookup.

The sampled grid is capped at 128 columns, the NES's 256 native columns halved.
That is already wider than most terminals people run this in.
The render loop is nowhere near what limits the speed here.
It is still written the same way DOOM's is:

- no per-cell `printf`
- no command substitution in the hot path
- the whole frame built as one string and emitted with a single `printf`
- an SGR escape skipped whenever it repeats the previous *cell's* color

That is nearly free, and it reduces the string a lot on the NES's large flat-color areas.
The PPU draws from a fixed 64-entry palette built into the chip.
At most 25 colors are on screen at once.

## Loading the ROM

Unlike DOOM's WAD, delivered through a host import, the NES module has zero wasm imports.
The frontend allocates a buffer with `allocRom(size)` and copies the ROM bytes into it directly.
That takes a few seconds of array assignments for a ~42KB ROM, once, at start.
The frontend then calls `initGame()`, which hands the buffer to `agnes`.

## Controls

Terminals deliver key *presses* only, never releases.
There is also no meaningful "held key" at this frame rate.
So the keys pressed since the last tick are folded into a single controller bit mask.
`setInput` is called with it just before `tickGame`.
The next frame starts from an empty set again.
An unpressed key becomes `setInput(0)` on the following frame.
One tick of "held down," as fine-grained as input can get at tens of seconds per frame.

| Key | NES button (bit) |
| --- | --- |
| Arrow keys | D-pad (Up 16, Down 32, Left 64, Right 128) |
| x | A (1) |
| z | B (2) |
| Enter | Start (8) |
| Space | Select (4) |
| q / Ctrl-C | Exit (terminal restored on exit) |

The button bits match [`nes_demo.c`](../../apps/src/nes_demo.c)'s `setInput`.
There is no sound: the module exposes no audio interface.

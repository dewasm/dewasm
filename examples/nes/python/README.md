# NES (Python, ANSI terminal)

An interactive NES frontend that renders into the terminal instead of a window (see `../go` for the pixel-window frontend), the same technique the [DOOM Python frontend](../../doom/python/) uses.
`build.sh` builds `cache/nes.wasm` (an [agnes](https://github.com/kgabis/agnes)-based emulator wrapped by `examples/apps/src/nes_demo.c`) via `examples/apps/scripts/nes.sh` and converts it to Python with dewasm (`nes_gen.py`, gitignored, regenerated on every build).
Unlike `../../doom`, `nes.wasm` has **zero host imports** -- no console messages, no save games, no clock -- so `main.py` only loads a ROM into the module's linear memory and drives the game loop itself: pacing, input polling, and frame presentation are entirely the host's job (the module has no clock import of its own to pace against, unlike DOOM's internal 35Hz timer).
Stdlib only: no third-party packages, nothing to `pip install`.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you quit.
`./run.sh --smoke` instead runs a headless self-check: it loads the ROM, ticks the emulator 40 times with no input, sanity-checks the last frame, writes it to `screenshot.ppm` (binary P6 PPM -- stdlib has no PNG encoder), and exits non-zero on failure.
Either mode takes an optional ROM path as the last argument (`./run.sh path/to/game.nes` or `./run.sh --smoke path/to/game.nes`); the default is the bundled demo ROM, `examples/apps/cache/alter_ego.nes`.

## Honest performance

**Measured ~1.1-1.3 frames/sec (headless `--smoke`, on an Apple Silicon laptop)** -- against the NES's ~60Hz frame rate, in the same range as the [DOOM Python frontend](../../doom/python/)'s ~1.3 ticks/sec; not playable, movement reads as a slideshow.
The tick is always the bottleneck, so the 60Hz pacing sleep never fires in practice.

## Rendering

The module hands over agnes's own frame representation, one palette *index* per pixel (masked with `0x3f`) plus the fixed 64-entry palette, rather than a rendered image, drawn as native-resolution (no upscaling, unlike DOOM's 2x) half-block characters with unchanged cells and repeated escape codes skipped -- the same reference pattern shared with the sibling frontends.

## Controls

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| x | A |
| z | B |
| Enter | Start |
| Space | Select |
| q / Ctrl-C | Quit |

Terminals deliver key *presses* only, never releases, so -- like the DOOM Python frontend -- each press keeps a button held in the `setInput` bitmask for 400ms after the last matching press/autorepeat: wider than the Ruby/Perl NES frontends use, because this backend's ticks land roughly a second apart (see "Honest performance" above), so the window has to bridge the gap between ticks, not just a terminal's own autorepeat interval.
`setInput` wants this bitmask (not discrete down/up events) on every tick, unlike DOOM's edge-triggered `reportKeyDown`/`reportKeyUp` pair.

Terminal state (raw mode, alternate screen, cursor visibility) is always restored on exit, including on Ctrl-C.

The bundled ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, released into the public domain.

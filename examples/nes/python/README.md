# NES (Python, ANSI terminal)

An interactive NES frontend that renders into the terminal instead of a window.
See `../go` for the frontend that draws pixels in a window.
It uses the technique of the [DOOM Python frontend](../../doom/python/).
`build.sh` builds `cache/nes.wasm` via `examples/apps/scripts/nes.sh`.
That module is an emulator built on [`agnes`](https://github.com/kgabis/agnes), wrapped by `examples/apps/src/nes_demo.c`.
`build.sh` converts it to Python with dewasm, into `nes_gen.py`.
That file is ignored by Git and regenerated on every build.
Unlike `../../doom`, `nes.wasm` has **zero host imports**: no console messages, no save games, no clock.
So `main.py` only loads a ROM into the module's linear memory and drives the game loop itself.
Pacing, input polling, and frame presentation are entirely the host's job.
The module has no clock import of its own to pace against, unlike DOOM's internal 35Hz timer.
Standard library only: no third-party packages, nothing to `pip install`.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you exit.
`./run.sh --smoke` instead runs a headless self-check.
It loads the ROM and ticks the emulator 40 times with no input.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary P6 PPM, since the standard library has no PNG encoder.
Either mode takes an optional ROM path as the last argument.
Examples are `./run.sh path/to/game.nes` and `./run.sh --smoke path/to/game.nes`.
The default is the bundled demonstration ROM, `examples/apps/cache/alter_ego.nes`.
Both modes run under `pypy3` when it is on `PATH` and under `python3` otherwise.
The reason is below.
Set `PYTHON` to pick an interpreter explicitly (`PYTHON=python3 ./run.sh --smoke`).

## Honest performance

**Measured ~11 frames/sec under PyPy 7.3 and ~2.1-2.2 under CPython 3.14.**
Both ran headless (`--smoke`) on an Apple Silicon laptop.
The NES frame rate is ~60Hz.
`run.sh` prefers PyPy, but even there this is not playable.
Movement looks like a slideshow.
The [DOOM Python frontend](../../doom/python/) runs ~45 ticks/sec under PyPy, above DOOM's rate of 35Hz.
Unlike it, this one stays well under its target rate on both interpreters.
The tick always limits the speed, so the 60Hz pacing sleep never fires in practice.

## Rendering

The module hands over the frame representation `agnes` uses internally, not a rendered image.
That is one palette *index* per pixel (masked with `0x3f`), plus the fixed 64-entry palette.
The frame is drawn as native-resolution half-block characters, with no scaling up, unlike DOOM's 2x.
Unchanged cells and repeated escape codes are skipped.
This is the reference pattern shared with the related frontends.

## Controls

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| x | A |
| z | B |
| Enter | Start |
| Space | Select |
| q / Ctrl-C | Exit |

Terminals deliver key *presses* only, never releases.
So, like the DOOM Python frontend, each press keeps a button held in the `setInput` bit mask.
The button stays held for 400ms after the last matching press or key repeat.
That is wider than the Ruby/Perl NES frontends use, because this backend's ticks land far apart.
Ticks are ~90ms apart under PyPy and ~0.5s under CPython; see "Honest performance" above.
So the window has to bridge the gap between ticks, not just a terminal's own key-repeat interval.
`setInput` wants this bit mask on every tick, not discrete down/up events.
That differs from DOOM's edge-triggered `reportKeyDown`/`reportKeyUp` pair.

Terminal state is always restored on exit, including on Ctrl-C.
That state is raw mode, the alternate screen, and cursor visibility.

The bundled ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, released into the public domain.

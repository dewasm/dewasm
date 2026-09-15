# NES (Codon, ANSI terminal)

An interactive NES frontend that renders into the terminal instead of a window (see `../go` for the pixel-window frontend), the same technique the [Python NES frontend](../python/) uses, compiled ahead of time by the [Codon](https://github.com/exaloop/codon) toolchain.
`build.sh` builds `cache/nes.wasm` (an [agnes](https://github.com/kgabis/agnes)-based emulator wrapped by `examples/apps/src/nes_demo.c`) via `examples/apps/scripts/nes.sh`, converts it to Codon with dewasm (`nes_gen.codon`, gitignored, regenerated on every build), and compiles library and frontend into one native binary.
Unlike `../../doom`, `nes.wasm` has **zero host imports** (no console messages, no save games, no clock), so `main.codon` only loads a ROM into the module's linear memory and drives the game loop itself: pacing, input polling, and frame presentation are entirely the host's job (the module has no clock import of its own to pace against, unlike DOOM's internal 35Hz timer).

Codon is a statically typed Python dialect, not CPython, and its standard library has no `termios`, `select`, `tty` or `os.get_terminal_size`.
The terminal is driven through libc instead: `poll` for non-blocking key reads, `tcgetattr`/`cfmakeraw`/`tcsetattr` for raw mode, and `ioctl(TIOCGWINSZ)` for the terminal size.
Nothing else is needed: no third-party packages.

Codon also has no import path for a sibling source file, so `build.sh` links the frontend against the generated library by concatenating the two into `nes_app.codon` and compiling that, which is how the backend's own test and benchmark runners build generated code.
The compile is skipped when that concatenated source is byte-identical to the one the existing binary was built from.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you quit.
`./run.sh --smoke` instead runs a headless self-check: it loads the ROM, ticks the emulator 40 times with no input, sanity-checks the last frame, writes it to `screenshot.ppm` (binary P6 PPM, the same format the cross-backend frame snapshot is pinned in), and exits non-zero on failure.
Either mode takes an optional ROM path as the last argument (`./run.sh path/to/game.nes` or `./run.sh --smoke path/to/game.nes`); the default is the bundled demo ROM, `examples/apps/cache/alter_ego.nes`.

`codon` 0.20 or newer has to be on `PATH`; `DEWASM_CODON` names it explicitly.
`run.sh` puts the toolchain's `lib/codon` directory on the loader path, which a `codon build` binary needs for Codon's runtime shared libraries.

## Honest performance

**Measured ~400-420 frames/sec headless (`--smoke`, `-release`, on an Apple Silicon laptop)**, against the NES's ~60Hz frame rate: about 7x the frame rate the emulator needs, so the fixed-timestep sleep does the pacing and the interactive status line sits at 59.9-60.1 frames/sec.
That is roughly 36x the [Python frontend](../python/)'s ~11 frames/sec under PyPy, and the first NES terminal frontend here with headroom to spare rather than a deficit.

Build mode is the one real trade-off, both numbers measured on the same machine over the same concatenated source (about 5,500 lines, nearly all of it generated):

| `codon build` | Compile | Smoke frame rate | 49x23-cell render, to a string |
| --- | --- | --- | --- |
| `-release` (default) | ~12.8s | ~400-420 frames/sec | ~0.02ms |
| debug (`CODON_BUILD=debug`) | ~7.1s | ~48-49 frames/sec | ~0.18ms |

`-release` is the default because the debug build lands *under* the 60Hz target while costing only 1.8x the compile time to clear it 7x over.
`CODON_BUILD=debug ./run.sh` is there for editing `main.codon`, where the shorter compile matters more than the frame rate.

## Rendering

The module hands over agnes's own frame representation, one palette *index* per pixel (masked with `0x3f`) plus the fixed 64-entry palette, rather than a rendered image, drawn as native-resolution (no upscaling, unlike DOOM's 2x) half-block characters with unchanged cells and repeated escape codes skipped, the same reference pattern shared with the sibling frontends.

## Controls

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| x | A |
| z | B |
| Enter | Start |
| Space | Select |
| q / Ctrl-C | Quit |

Terminals deliver key *presses* only, never releases, so (like the Python NES frontend) each press keeps a button held in the `setInput` bitmask for 400ms after the last matching press/autorepeat.
`setInput` wants this bitmask (not discrete down/up events) on every tick, unlike DOOM's edge-triggered `reportKeyDown`/`reportKeyUp` pair.

Raw mode delivers Ctrl-C as a byte on stdin rather than as a signal, so the quit path is the same one `q` takes.
Terminal state is restored from the exact `tcgetattr` snapshot taken at startup, on every exit path.

The bundled ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, released into the public domain.

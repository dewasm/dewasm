# NES (Codon, ANSI terminal)

An interactive NES frontend that renders into the terminal instead of a window.
See `../go` for the frontend that draws pixels in a window.
It uses the technique of the [Python NES frontend](../python/).
The [Codon](https://github.com/exaloop/codon) toolchain compiles it ahead of time.
`build.sh` builds `cache/nes.wasm` via `examples/apps/scripts/nes.sh`.
That module is an emulator built on [`agnes`](https://github.com/kgabis/agnes), wrapped by `examples/apps/src/nes_demo.c`.
`build.sh` converts it to Codon with dewasm, into `nes_gen.codon`.
That file is ignored by Git and regenerated on every build.
`build.sh` then compiles library and frontend into one native binary.
Unlike `../../doom`, `nes.wasm` has **zero host imports**: no console messages, no save games, no clock.
So `main.codon` only loads a ROM into the module's linear memory and drives the game loop itself.
Pacing, input polling, and frame presentation are entirely the host's job.
The module has no clock import of its own to pace against, unlike DOOM's internal 35Hz timer.

Codon is a statically typed Python dialect, not CPython.
Its standard library has no `termios`, `select`, `tty` or `os.get_terminal_size`.
The terminal is driven through `libc` instead:

- `poll` for non-blocking key reads
- `tcgetattr`/`cfmakeraw`/`tcsetattr` for raw mode
- `ioctl(TIOCGWINSZ)` for the terminal size

Nothing else is needed: no third-party packages.

Codon also has no import path for a source file in the same directory.
So `build.sh` joins the frontend and the generated library into `nes_app.codon`.
It then compiles that file.
The backend's own test and benchmark runners build generated code the same way.
If the joined source is byte-identical to the existing binary's, the compile is skipped.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you exit.
`./run.sh --smoke` instead runs a headless self-check.
It loads the ROM and ticks the emulator 40 times with no input.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary P6 PPM, the same format the cross-backend frame snapshot is stored in.
Either mode takes an optional ROM path as the last argument.
Examples are `./run.sh path/to/game.nes` and `./run.sh --smoke path/to/game.nes`.
The default is the example ROM, `examples/apps/cache/alter_ego.nes`.

`codon` 0.20 or newer has to be on `PATH`; `DEWASM_CODON` names it explicitly.
`run.sh` puts the toolchain's `lib/codon` directory on the loader path.
A `codon build` binary needs it for Codon's runtime shared libraries.

## Honest performance

**Measured ~400-420 frames/sec headless (`--smoke`, `-release`, on an Apple Silicon laptop).**
The NES frame rate is ~60Hz, so this is about 7x the frame rate the emulator needs.
So the pacing is a sleep that holds each frame to a fixed time step.
So the interactive status line sits at 59.9-60.1 frames/sec.
That is roughly 36x the [Python frontend](../python/)'s ~11 frames/sec under PyPy.
It is the first NES terminal frontend here with speed to spare rather than too little.

Build mode is the one real trade-off.
Both numbers were measured on the same machine over the same joined source.
That source is about 5,500 lines, nearly all of it generated:

| `codon build` | Compile | Smoke frame rate | 49x23-cell render, to a string |
| --- | --- | --- | --- |
| `-release` (default) | ~12.8s | ~400-420 frames/sec | ~0.02ms |
| debug (`CODON_BUILD=debug`) | ~7.1s | ~48-49 frames/sec | ~0.18ms |

`-release` is the default because the debug build lands *under* the 60Hz target.
The release build costs only 1.8x the compile time to clear it 7x over.
`CODON_BUILD=debug ./run.sh` is there for editing `main.codon`.
There the shorter compile matters more than the frame rate.

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
So, like the Python NES frontend, each press keeps a button held in the `setInput` bit mask.
The button stays held for 400ms after the last matching press or key repeat.
`setInput` wants this bit mask on every tick, not discrete down/up events.
That differs from DOOM's edge-triggered `reportKeyDown`/`reportKeyUp` pair.

Raw mode delivers Ctrl-C as a byte on `stdin` rather than as a signal.
So the exit path is the same one `q` takes.
Terminal state is restored from the exact `tcgetattr` snapshot taken at start, on every exit path.

The example ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, released into the public domain.

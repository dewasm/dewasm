# DOOM (Codon, ANSI terminal)

An interactive frontend for the DOOM shareware episode that renders straight into the terminal.
It needs no window and no GPU.
`build.sh` fetches jacobenget/doom.wasm, checksum-pinned into the shared apps cache.
It converts the module to Codon with dewasm, into `doom_gen.codon`.
That file is ~10MB, gitignored, and regenerated on every build.
`build.sh` then compiles it together with the host program in `main.codon` into one native binary.
`main.codon` implements the module's ten host imports.
They cover console messages, save-game files, the game clock, and frame delivery.
`main.codon` also draws the framebuffer as 24-bit-color half-blocks.
It reads keys from the terminal in raw mode.

This is the [Python frontend](../python/) compiled ahead of time instead of interpreted.
[Codon](https://github.com/exaloop/codon) is a statically typed Python dialect, not CPython.
Its standard library has no `termios`, `select`, `tty` or `os.get_terminal_size`.
The terminal is driven through libc instead:

- `poll` for non-blocking key reads
- `tcgetattr`/`cfmakeraw`/`tcsetattr` for raw mode
- `ioctl(TIOCGWINSZ)` for the terminal size

A host import is a `DoomRt.Fn` subclass rather than a closure, for the same reason.
Nothing else is needed: no third-party packages.

Codon also has no import path for a sibling source file.
So `build.sh` concatenates the frontend and the generated library into `doom_app.codon`.
It then compiles that file.
The backend's own test and benchmark runners build generated code the same way.

## Build time

The concatenated source is about 53,000 lines, nearly all of it generated.
A debug `codon build` of it takes about 26 seconds on an Apple Silicon laptop, 8 of them the parse.
It produces a 17MB binary.

`build.sh` caches the result.
The concatenated source is compared byte for byte with the source of the existing binary.
An unchanged source skips the compile entirely.
The default is the cheaper debug build.
`CODON_BUILD=release ./build.sh` selects the optimized build.
That build costs about 4.5 minutes on the same source.
Almost all of that is Codon's IR capture analysis, run once per folding round.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you quit.
On a clean checkout, that first line converts the module and compiles the result.
This takes about half a minute once dewasm itself is built.
`./run.sh --smoke` instead runs a headless self-check.
It inits the game and ticks it 15 times with no terminal takeover.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary P6, the same format the cross-backend frame snapshot is pinned in.

`codon` 0.20 or newer has to be on `PATH`; `DEWASM_CODON` names it explicitly.
`run.sh` puts the toolchain's `lib/codon` directory on the loader path.
A `codon build` binary needs it for Codon's runtime shared libraries.

## Honest performance

**The tick rate is unmeasured.**
The binary builds and plays, but no rate is quoted here.
None was taken under a method worth quoting.
The `--smoke` self-check times 15 ticks.
How much the game simulates per tick depends on the wall clock it reads.
So the same binary reports anywhere from 9 to 66 ticks/sec across consecutive runs.
Nothing about the other frontends' rates transfers.
`../python/` runs ~45 ticks/sec under PyPy, and `../../nes/codon/` runs ~400 frames/sec.
Those are different interpreters on different modules.
Extrapolating either to this one would be a guess dressed as a measurement.

What was verified is the frontend's own logic.
It was verified by compiling `main.codon` against a hand-written stub and driving that.
The stub exposes the same `Doom`/`DoomRt` API surface as the generated library:

- the same boxed `Extern`/`Val`/`Fn` boundary
- the same `KEY_*` globals
- the same `B,G,R,A` framebuffer in linear memory

Driving the stub verified the following:

- The smoke path writes a well-formed P6 `screenshot.ppm` from the framebuffer.
  It also applies its distinct-color check.
- The save-game imports round trip.
  `writeSaveGame` creates `.savegame/` and returns the length, and `sizeOfSaveGame` reports it.
  `readSaveGame` returns it, and the bytes land back in linear memory unchanged.
  An absent save id returns 0 from both rather than raising.
- Key decoding and the edge-triggered hold logic are correct under a pty.
  The keys are arrows, `f`, space, comma, period, Tab, a lone Escape, and face-value letters and digits.
  Each reaches `reportKeyDown` once with the module's own key code.
  Each is followed by exactly one `reportKeyUp` after the release window.
- Both quit paths (`q` and Ctrl-C) exit cleanly.
  They restore the alternate screen and cursor visibility.
  Nothing is emitted after the terminal is handed back.

That covers everything this directory owns.
It does not cover the generated library.
The cross-backend spec and framebuffer-snapshot tests cover that instead.

The assembled binary passes the same `--smoke` check against the real generated library.
The game inits, ticks, and hands over a 640x400 frame with 240 distinct colors.

## Rendering

Each character cell shows two vertically-stacked pixels via the upper-half-block character `▀`.
The foreground color is the top pixel, and the background color is the bottom pixel.
Both are set with 24-bit truecolor escapes.
DOOM's 640x400 framebuffer is a 2x upscale of its native 320x200.
So pixels are sampled from that logical 320x200 grid.
They are nearest-neighbor-fit to however many columns/rows the terminal actually has.
The fit is capped at 320 columns, and one row is reserved for the status line.
Only cells that changed since the previous frame are redrawn.
Repeated colors within a redrawn run don't re-emit their escape code.

## Controls

- Arrow keys: move / turn
- `f`: fire (Ctrl isn't something a terminal can deliver as a distinct keypress)
- Space: use / open doors
- Comma / period: strafe left / right
- Tab: automap
- Escape / Enter / Backspace: menus
- Other letters and digits: text entry and prompts (`y` confirms, digits select weapons).
  They are taken at face value as lowercase ASCII.
- `q` or Ctrl-C: quit

Terminals only report key-down events, never key-up, so a release is synthesized.
After a keypress, `reportKeyUp` fires automatically once ~400ms pass without seeing that key again.
Autorepeat from holding a key down keeps extending the deadline.
Raw mode delivers Ctrl-C as a byte on stdin rather than as a signal.
So the quit path is the same one `q` takes.
Terminal state is restored from the exact `tcgetattr` snapshot taken at startup, on every exit path.

Save games are written to `.savegame/` (gitignored) relative to wherever the script runs.

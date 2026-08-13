# DOOM (Python, ANSI terminal)

An interactive frontend for the DOOM shareware episode that renders straight into the terminal: no window, no GPU.
`build.sh` fetches jacobenget/doom.wasm (checksum-pinned into the shared apps cache) and converts it to Python with dewasm (`doom_gen.py`, ~11MB, gitignored, regenerated on every build) and links it against a small host program in `main.py` that implements the module's ten host imports (console messages, save-game files, the game clock, and frame delivery), draws the framebuffer as 24-bit-color half-blocks, and reads keys from the terminal in raw mode.
Stdlib only: no third-party packages, nothing to `pip install`.

## Run

```sh
./run.sh
```

takes over the terminal (alternate screen, hidden cursor, raw input) and starts rendering.
`./run.sh --smoke` instead runs a headless self-check: it inits the game, ticks it 15 times with no terminal takeover, sanity-checks the last frame, writes it to `screenshot.ppm` (binary P6: stdlib has no PNG encoder), and exits non-zero on failure.
Both modes run under `pypy3` when it is on `PATH` and under `python3` otherwise, for the reason below; set `PYTHON` to pick an interpreter explicitly (`PYTHON=python3 ./run.sh --smoke`).

## Honest performance

**Measured ~45 ticks/sec under PyPy 7.3 and ~1.2-1.8 under CPython 3.14, headless, on an Apple Silicon laptop**, against DOOM's own internal tic rate of 35Hz.
PyPy's JIT clears that tic rate, which is why `run.sh` prefers it: a playable game, between Ruby with YJIT's ~15 (`../ruby/`) and the 55-70 of the Go/Java frontends (`../go/`, `../java/`).
CPython has no JIT and interprets the generated source line by line, and DOOM's software renderer and game logic are thousands of lines of hot loops per tic, so there movement reads as a slideshow, not motion.
The self-check is only 15 ticks, short enough that PyPy's warm-up (about a second) sometimes falls inside it and the reported rate drops to ~10.

It's still worth running under either: the same unmodified wasm binary and the same dewasm-generated library that plays smoothly elsewhere comes out the other side of a stdlib Python interpreter rendering actual DOOM frames, as colored terminal text.
That id Software's 1993 engine executes at all through this path, entirely in printable ANSI escape codes, is the point; frame rate isn't.

## Rendering

Each character cell shows two vertically-stacked pixels via the upper-half-block character `▀`: the foreground color is the top pixel, the background color is the bottom pixel, both set with 24-bit truecolor escapes.
DOOM's 640x400 framebuffer is a 2x upscale of its native 320x200, so pixels are sampled from that logical 320x200 grid and nearest-neighbor-fit to however many columns/rows the terminal actually has (capped at 320 columns, one row reserved for the status line).
Only cells that changed since the previous frame are redrawn, and repeated colors within a redrawn run don't re-emit their escape code.
Under CPython the frame rate is far too low for this to matter; under PyPy a tick costs ~22ms and a full redraw of a 73x23-cell frame ~8ms, so it does, and either way it keeps a slow link (e.g. ssh) usable.

## Controls

- Arrow keys: move / turn
- `f`: fire (Ctrl isn't something a terminal can deliver as a distinct keypress)
- Space: use / open doors
- Comma / period: strafe left / right
- Tab: automap
- Escape / Enter / Backspace: menus
- Other letters and digits: text entry and prompts (`y` confirms, digits select weapons), taken at face value as lowercase ASCII
- `q` or Ctrl-C: quit

Terminals only report key-down events, never key-up, so a release is synthesized: after a keypress, `reportKeyUp` fires automatically once ~400ms pass without seeing that key again (autorepeat from holding a key down keeps extending the deadline).
That window is wider than a typical key-repeat gap because under CPython this backend manages under two ticks/sec, so the game barely gets a chance to notice a repeat before the next poll.

Save games are written to `.savegame/` (gitignored) relative to wherever the script runs.

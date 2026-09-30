# DOOM (Python, ANSI terminal)

An interactive frontend for the DOOM shareware episode that renders straight into the terminal.
It needs no window and no GPU.
`build.sh` fetches `jacobenget/doom.wasm`, checked against a fixed checksum, into the shared apps cache.
It converts the module to Python with dewasm, into `doom_gen.py`.
That file is ~11MB, ignored by Git, and regenerated on every build.
`build.sh` then links it against a small host program in `main.py`.
The host program implements the module's ten host imports.
They cover console messages, save-game files, the game clock, and frame delivery.
The host program also draws the framebuffer as 24-bit-color half-blocks.
It reads keys from the terminal in raw mode.
Standard library only: no third-party packages, nothing to `pip install`.

## Run

```sh
./run.sh
```

takes over the terminal (alternate screen, hidden cursor, raw input) and starts rendering.
`./run.sh --smoke` instead runs a headless self-check.
It initializes the game and ticks it 15 times without taking over the terminal.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary P6, since the standard library has no PNG encoder.
Both modes run under `pypy3` when it is on `PATH` and under `python3` otherwise.
The reason is below.
Set `PYTHON` to pick an interpreter explicitly (`PYTHON=python3 ./run.sh --smoke`).

## Honest performance

**Measured ~45 ticks/sec under PyPy 7.3 and ~1.2-1.8 under CPython 3.14.**
Both ran headless on an Apple Silicon laptop.
DOOM's own internal tic rate is 35Hz.
PyPy's JIT clears that tic rate, which is why `run.sh` prefers it.
Under PyPy the game is playable.
The rate sits between Ruby with YJIT (~15, `../ruby/`) and Go/Java (55-70, `../go/`, `../java/`).
CPython has no JIT and interprets the generated source line by line.
DOOM's software renderer and game logic are thousands of lines of hot loops per tic.
So under CPython, movement looks like a slideshow, not motion.
The self-check is only 15 ticks.
That is short enough that PyPy's warm-up (about a second) sometimes falls inside it.
The reported rate then drops to ~10.

It's still worth running under either interpreter.
The same unmodified wasm binary and the same dewasm-generated library play smoothly elsewhere.
With only Python's standard library, they render actual DOOM frames as colored terminal text.
The point is that id Software's 1993 engine executes at all through this path.
It renders entirely in printable ANSI escape codes.
Frame rate is not the point.

## Rendering

Each character cell shows two vertically-stacked pixels via the upper-half-block character `▀`.
The text color is the top pixel, and the background color is the bottom pixel.
Both are set with 24-bit color escapes.
DOOM's 640x400 framebuffer is its native 320x200, scaled by 2.
So pixels are sampled from that logical 320x200 grid.
They are nearest-neighbor-fit to however many columns/rows the terminal actually has.
The fit is capped at 320 columns, and one row is reserved for the status line.
Only cells that changed since the previous frame are redrawn.
Repeated colors within a redrawn run don't re-emit their escape code.
Under CPython the frame rate is far too low for this to matter.
Under PyPy a tick costs ~22ms, and a full redraw of a 73x23-cell frame ~8ms.
So there it does matter.
Either way, it keeps a slow link (for example, SSH) usable.

## Controls

- Arrow keys: move / turn
- `f`: fire (Ctrl isn't something a terminal can deliver as a distinct key press)
- Space: use / open doors
- Comma / period: strafe left / right
- Tab: automap
- Escape / Enter / Backspace: menus
- Other letters and digits: text entry and prompts (`y` confirms, digits select weapons).
  They are taken at face value as lower-case ASCII.
- `q` or Ctrl-C: exit

Terminals only report key-down events, never key-up, so a release is generated.
After a key press, `reportKeyUp` fires automatically once ~400ms pass without seeing that key again.
Key repeat from holding a key down keeps extending the deadline.
That window is wider than a typical key-repeat gap.
Under CPython this backend manages under two ticks/sec.
So the game barely gets a chance to notice a repeat before the next poll.

Save games are written to `.savegame/` (ignored by Git) relative to wherever the script runs.

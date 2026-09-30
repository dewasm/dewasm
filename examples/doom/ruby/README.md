# DOOM (Ruby, ANSI terminal)

An interactive DOOM frontend that renders into the terminal instead of a window.
See `../go` and `../java` for the frontends that draw pixels in a window.
`build.sh` fetches `jacobenget/doom.wasm`, fixed by checksum, into the shared apps cache.
It converts the module to Ruby with dewasm, into `doom_gen.rb`.
That file is ~11MB, ignored by Git, and regenerated on every build.
`main.rb` implements the module's ten host imports.
They cover console logging, save-game I/O, the game clock, and frame delivery.
`main.rb` also has a terminal renderer and raw-mode key input.
A terminal is not the worse choice here.
The Ruby backend only manages ~15 ticks/sec under YJIT, far below what a GUI needs.
That rate is plenty for a terminal.
A terminal has orders of magnitude fewer cells to redraw than a window has pixels.
The [`gui/`](gui/) directory runs the same generated library in a real window with Gosu.
It shares this directory's `doom_gen.rb`.
What it buys is real key releases, which no terminal can report.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you exit.
`./run.sh --smoke` instead runs a headless self-check.
It initializes the game and ticks it 60 times without taking over the terminal.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary PPM, since Ruby's standard library has no PNG writer.

## Rendering

DOOM's 640x400 framebuffer is its native 320x200, scaled by 2.
It is scaled down to fit the terminal.
The half-block trick draws two source pixels per character cell.
Each cell is `▀` colored via 24-bit color SGR.
`\e[38;2;R;G;Bm` sets the text color (top pixel), and `\e[48;2;R;G;Bm` the background (bottom pixel).
Target width is `min(terminal columns, 320)`.
Height in cells follows from that at DOOM's aspect ratio, minus one row for the status line.
At a typical 160-column terminal that's 160x100 logical pixels, i.e. 160x50 character cells.

Only changed cells are redrawn.
DOOM's software renderer is paletted (VGA Mode 13h, ≤256 colors).
So most cells repeat exactly frame to frame.
An SGR code is skipped whenever a cell's color matches the previous cell's.
The whole frame is built as one string and written with a single `write` call.
This diffing and escape-sequence tracking is the actual performance-sensitive part of this frontend.
The wasm execution is not.

Measurements ran on an Apple Silicon Mac, headless (`--smoke`, 160x50 cells, under `ruby --yjit`).
**The frontend runs 15.9 ticks/sec without rendering, and 15.8 ticks/sec with it.**
That is about 0.5ms/frame of render overhead against a ~63ms/frame tick budget.
Rendering therefore costs well under 1% of the frame.
Without YJIT the Ruby backend drops to roughly 1 tick/sec, which is not playable.
`run.sh` always passes `--yjit`.
`main.rb` warns on `stderr` if it ends up running without it anyway.

## Controls

Terminals deliver key *presses* only, never releases.
So each press generates a `reportKeyDown` immediately.
A `reportKeyUp` follows once ~180ms pass with no repeat.
That is comfortably above a terminal's own key-repeat interval, so held keys stay held.

| Key | Action |
| --- | --- |
| Arrow keys | Move / turn |
| f | Fire (Ctrl isn't deliverable through a terminal) |
| Space | Use (open doors, flip switches) |
| , / . | Strafe left / right |
| Tab | Automap |
| Enter | Menu confirm |
| Escape | Menu / pause |
| Backspace | Menu back |
| 0-9 | Weapon select / text entry |
| y / n | Confirm prompts |
| q / Ctrl-C | Exit |

Shift (run) has no terminal equivalent and isn't mapped.

Save games are written to `.savegame/` (ignored by Git) relative to wherever the script runs.

# DOOM (Ruby, ANSI terminal)

An interactive DOOM frontend that renders into the terminal instead of a window (see `../go` and `../java` for the pixel-window frontends).
`build.sh` fetches jacobenget/doom.wasm (checksum-pinned into the shared apps cache) and converts it to Ruby with dewasm (`doom_gen.rb`, ~11MB, gitignored, regenerated on every build) and `main.rb` implements the module's ten host imports (console logging, save-game I/O, the game clock, and frame delivery) plus a terminal renderer and raw-mode keyboard input.
A terminal is not a downgrade here: the Ruby backend only manages ~15 ticks/sec under YJIT, far below what a GUI needs but plenty for a terminal, which has orders of magnitude fewer cells to redraw than a window has pixels.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you quit.
`./run.sh --smoke` instead runs a headless self-check: it inits the game, ticks it 60 times with no terminal takeover, sanity-checks the last frame, writes it to `screenshot.ppm` (binary PPM — Ruby's stdlib has no PNG writer), and exits non-zero on failure.

## Rendering

DOOM's 640x400 framebuffer (a 2x upscale of its native 320x200) is downsampled to fit the terminal and drawn two source pixels per character cell with the half-block trick: `▀` colored via 24-bit truecolor SGR, `\e[38;2;R;G;Bm` for the foreground (top pixel) and `\e[48;2;R;G;Bm` for the background (bottom pixel).
Target width is `min(terminal columns, 320)`; height in cells follows from that at DOOM's aspect ratio, minus one row for the status line.
At a typical 160-column terminal that's 160x100 logical pixels, i.e. 160x50 character cells.

Only changed cells are redrawn — DOOM's software renderer is paletted (VGA Mode 13h, ≤256 colors), so most cells repeat exactly frame to frame — an SGR code is skipped whenever a cell's color matches the previous cell's, and the whole frame is built as one string and written with a single `write` call.
This diffing/escape-sequence bookkeeping is the actual performance-sensitive part of this frontend; the wasm execution is not.

Measured on an Apple Silicon laptop, headless (`--smoke`, 160x50 cells, under `ruby --yjit`): **15.9 ticks/sec bare, 15.8 ticks/sec with terminal rendering included** — about 0.5ms/frame of render overhead against a ~63ms/frame tick budget, i.e. rendering costs well under 1% of the frame.
Without YJIT the Ruby backend drops to roughly 1 tick/sec, which is not playable; `run.sh` always passes `--yjit`, and `main.rb` warns on stderr if it ends up running without it anyway.

## Controls

Terminals deliver key *presses* only, never releases, so each press synthesizes a `reportKeyDown` immediately and a `reportKeyUp` once ~180ms pass with no repeat (comfortably above a terminal's own autorepeat interval, so held keys stay held).

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
| q / Ctrl-C | Quit |

Shift (run) has no terminal equivalent and isn't mapped.

Save games are written to `.savegame/` (gitignored) relative to wherever the script runs.

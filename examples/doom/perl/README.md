# DOOM (Perl, ANSI terminal)

An interactive frontend for the DOOM shareware episode that renders straight into the terminal: no window, no GPU (see `../go` and `../java` for the pixel-window frontends).
`build.sh` fetches jacobenget/doom.wasm (checksum-pinned into the shared apps cache) and converts it to Perl with dewasm (`doom_gen.pl`, ~12MB, gitignored, regenerated on every build) and `main.pl` implements the module's ten host imports (console messages, save-game files, the game clock, and frame delivery), draws the framebuffer as 24-bit-color half-blocks, and reads keys from the terminal in raw mode.
Core modules only: no CPAN installs.
Raw mode goes through `stty` because `Term::ReadKey` is not core.

## Run

```sh
./run.sh
```

takes over the terminal (alternate screen, hidden cursor, raw input) and starts rendering.
`./run.sh --smoke` instead runs a headless self-check: it inits the game, ticks it 10 times with no terminal takeover, sanity-checks the last frame, writes it to `screenshot.ppm` (binary P6: Perl's core modules include no PNG encoder), and exits non-zero on failure.

## Honest performance

**Measured ~0.7 ticks/sec, headless, on an Apple Silicon laptop**, against DOOM's own internal tic rate of 35Hz, and below even Python's ~1.3.
DOOM's renderer is all integer math, so the usual Perl-backend cost center (float ops as sub calls) barely applies; what's left is that plain Perl has no JIT and every generated function call pays the backend's recursion-depth accounting.
This is not a playable game: it's a slideshow with a crosshair.

It's still worth running, for the same reason the Python frontend is: the same unmodified wasm binary that plays smoothly through Go and Java runs, unmodified, through a plain Perl interpreter and comes out the other side rendering actual DOOM frames as ANSI escape codes.
The terminal rendering itself costs ~6ms/frame, noise against a ~1.4s tick.

## Rendering

Each character cell shows two vertically-stacked pixels via the upper-half-block character `▀`: the foreground color is the top pixel, the background color is the bottom pixel, both set with 24-bit truecolor escapes.
DOOM's 640x400 framebuffer (a 2x upscale of its native 320x200) is downsampled to fit the terminal, capped at 320 columns, with one row reserved for the status line.
Only cells that changed since the previous frame are redrawn, and an SGR code is skipped whenever a cell's color matches the previous cell's.
At this tick rate the diffing is far from necessary, but it's the reference pattern shared with the Ruby/Python/Bash frontends and it keeps a slow link (e.g. ssh) usable.

## Controls

Same as the other terminal frontends: arrows move/turn, `f` fires (Ctrl isn't deliverable through a terminal), Space uses, `,`/`.` strafe, Tab automap, Escape/Enter/Backspace for menus, other letters/digits at face value (`y` confirms, digits select weapons), `q`/Ctrl-C quits.
Shift (run) has no terminal equivalent and isn't mapped.

Terminals only report key-down events, never key-up, so a release is synthesized: after a keypress, `reportKeyUp` fires automatically once ~400ms pass without seeing that key again (autorepeat keeps extending the deadline).
The window is wider than Ruby's 180ms for the same reason as Python's: at well under one tick/sec, polls are over a second apart, so a narrow window would release held keys between polls.

Save games are written to `.savegame/` (gitignored) relative to wherever the script runs.

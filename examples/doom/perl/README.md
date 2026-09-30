# DOOM (Perl, ANSI terminal)

An interactive frontend for the DOOM shareware episode that renders straight into the terminal.
It needs no window and no GPU.
See `../go` and `../java` for the frontends that draw pixels in a window.
`build.sh` fetches `jacobenget/doom.wasm`, checked against a fixed checksum, into the shared apps cache.
It converts the module to Perl with dewasm, into `doom_gen.pl`.
That file is ~12MB, ignored by Git, and regenerated on every build.
`main.pl` implements the module's ten host imports.
They cover console messages, save-game files, the game clock, and frame delivery.
`main.pl` also draws the framebuffer as 24-bit-color half-blocks.
It reads keys from the terminal in raw mode.
Core modules only: no CPAN installs.
Raw mode goes through `stty` because `Term::ReadKey` is not core.

## Run

```sh
./run.sh
```

takes over the terminal (alternate screen, hidden cursor, raw input) and starts rendering.
`./run.sh --smoke` instead runs a headless self-check.
It initializes the game and ticks it 10 times without taking over the terminal.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary P6, since Perl's core modules include no PNG encoder.

## Honest performance

**Measured ~0.7 ticks/sec, headless, on an Apple Silicon Mac.**
DOOM's own internal tic rate is 35Hz, and this is below even Python's ~1.3.
DOOM's renderer is all integer math.
So the usual Perl-backend cost center (float operations as `sub` calls) barely applies.
What's left is that plain Perl has no JIT.
Every generated function call also pays the backend's recursion-depth accounting.
This is not a playable game: it is a slideshow with an aiming mark.

It's still worth running, for the same reason the Python frontend is.
The same unmodified wasm binary plays smoothly through Go and Java.
It also runs, unmodified, through a plain Perl interpreter.
It comes out the other side rendering actual DOOM frames as ANSI escape codes.
The terminal rendering itself costs ~6ms/frame, noise against a ~1.4s tick.

## Rendering

Each character cell shows two vertically-stacked pixels via the upper-half-block character `▀`.
The text color is the top pixel, and the background color is the bottom pixel.
Both are set with 24-bit color escapes.
DOOM's 640x400 framebuffer is its native 320x200, scaled by 2.
It is scaled down to fit the terminal, capped at 320 columns.
One row is reserved for the status line.
Only cells that changed since the previous frame are redrawn.
An SGR code is skipped whenever a cell's color matches the previous cell's.
At this tick rate the diffing is far from necessary.
But it's the reference pattern shared with the Ruby/Python/Bash frontends.
It also keeps a slow link (for example, SSH) usable.

## Controls

Same as the other terminal frontends:

- Arrow keys: move / turn
- `f`: fire (Ctrl isn't deliverable through a terminal)
- Space: use
- `,` / `.`: strafe left / right
- Tab: automap
- Escape / Enter / Backspace: menus
- Other letters and digits: taken at face value (`y` confirms, digits select weapons)
- `q` / Ctrl-C: exit

Shift (run) has no terminal equivalent and isn't mapped.

Terminals only report key-down events, never key-up, so a release is generated.
After a key press, `reportKeyUp` fires automatically once ~400ms pass without seeing that key again.
Key repeat keeps extending the deadline.
The window is wider than Ruby's 180ms for the same reason as Python's.
At well under one tick/sec, polls are over a second apart.
So a narrow window would release held keys between polls.

Save games are written to `.savegame/` (ignored by Git) relative to wherever the script runs.

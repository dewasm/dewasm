# NES (Ruby, ANSI terminal)

An interactive NES frontend that renders into the terminal instead of a window.
See `../go` for the pixel-window frontend.
The [`gui/`](gui/) subdirectory runs the same generated library in a real window with gosu.
It shares this directory's `nes_gen.rb`.
Its real key releases feed `setInput`'s bitmask directly, instead of the hold-window synthesis below.
`build.sh` builds `cache/nes.wasm` via `examples/apps/scripts/nes.sh`.
That module is an [agnes](https://github.com/kgabis/agnes)-based emulator wrapped by `examples/apps/src/nes_demo.c`.
`build.sh` converts it to Ruby with dewasm, into `nes_gen.rb`.
That file is gitignored and regenerated on every build.
Unlike `../../doom`, `nes.wasm` has **zero host imports**, so the host implements none.
So `main.rb` only loads a ROM into the module's linear memory and drives the game loop itself.
Pacing, input polling, and frame presentation are entirely the host's job.
The module has no clock import of its own to pace against, unlike DOOM's internal 35Hz timer.

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you quit.
`./run.sh --smoke` instead runs a headless self-check.
It inits the game and ticks it 300 times with no input.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary PPM, since Ruby's stdlib has no PNG writer.
Either mode takes an optional ROM path as the last argument.
Examples are `./run.sh path/to/game.nes` and `./run.sh --smoke path/to/game.nes`.
The default is the bundled demo ROM, `examples/apps/cache/alter_ego.nes`.

## Rendering

The module hands over agnes's own frame representation instead of a rendered image.
That is one palette *index* per pixel at `screenOffset()`.
It comes with the fixed 64-entry palette at `paletteOffset()` (masked with `0x3f`).
The frame is drawn two source pixels per character cell with the half-block trick.
`../../doom/ruby` uses the same trick.
Unchanged cells are skipped.
The render overhead measurement ran on an Apple Silicon laptop, headless.
It used `--smoke`, 160x50 cells, under `ruby --yjit`.
Render overhead stays well under 1ms/frame, noise against the tick cost.
See the numbers `--smoke` prints on your machine.

Pacing targets 60Hz.
The NTSC NES's real rate is ~60.0988Hz, close enough that no calibration is needed.
The frontend sleeps when it's running ahead of schedule and never sleeps when it can't keep up.
So it plays as fast as the interpreter can sustain, instead of stalling behind a fixed budget.

## Controls

Terminals deliver key *presses* only, never releases.
So, like `../../doom/ruby`, each press keeps a button held in the `setInput` bitmask.
The button stays held for ~180ms after the last matching press/autorepeat.
That is comfortably above a terminal's own autorepeat interval.
Unlike DOOM, the module actually wants this bitmask every tick, not discrete down/up events.

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| x | A |
| z | B |
| Enter | Start |
| Space | Select |
| q / Ctrl-C | Quit |

The bundled ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, released into the public domain.

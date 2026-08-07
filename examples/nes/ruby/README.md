# NES (Ruby, ANSI terminal)

An interactive NES frontend that renders into the terminal instead of a window (see `../go` for the pixel-window frontend). `build.sh` builds `cache/nes.wasm` (an [agnes](https://github.com/kgabis/agnes)-based emulator wrapped by `examples/apps/src/nes_demo.c`) via `examples/apps/scripts/nes.sh` and converts it to Ruby with dewasm (`nes_gen.rb`, gitignored, regenerated on every build). Unlike `../../doom`, `nes.wasm` has **zero host imports** — there is nothing to wire up — so `main.rb` only loads a ROM into the module's linear memory and drives the game loop itself: pacing, input polling, and frame presentation are entirely the host's job (the module has no clock import of its own to pace against, unlike DOOM's internal 35Hz timer).

## Run

```sh
./run.sh
```

builds and takes over the terminal (alternate screen, hidden cursor, raw input) until you quit. `./run.sh --smoke` instead runs a headless self-check: it inits the game, ticks it 300 times with no input, sanity-checks the last frame, writes it to `screenshot.ppm` (binary PPM — Ruby's stdlib has no PNG writer), and exits non-zero on failure. Either mode takes an optional ROM path as the last argument (`./run.sh path/to/game.nes` or `./run.sh --smoke path/to/game.nes`); the default is the bundled demo ROM, `examples/apps/cache/alter_ego.nes`.

## Rendering

The module hands over agnes's own frame representation instead of a rendered image: one palette *index* per pixel at `screenOffset()`, plus the fixed 64-entry palette at `paletteOffset()` (masked with `0x3f`), drawn two source pixels per character cell with the half-block trick (the same one `../../doom/ruby` uses), with unchanged cells skipped. Measured on an Apple Silicon laptop, headless (`--smoke`, 160x50 cells, under `ruby --yjit`): render overhead stays well under 1ms/frame, noise against the tick cost (see the numbers `--smoke` prints on your machine).

Pacing targets 60Hz (the NTSC NES's real rate is ~60.0988Hz — close enough that no calibration is needed): the frontend sleeps when it's running ahead of schedule and never sleeps when it can't keep up, so it plays at the fastest rate the interpreter can sustain instead of stalling behind a fixed budget.

## Controls

Terminals deliver key *presses* only, never releases, so — like `../../doom/ruby` — each press keeps a button held in the `setInput` bitmask for ~180ms after the last matching press/autorepeat, comfortably above a terminal's own autorepeat interval, and unlike DOOM this bitmask (not discrete down/up events) is what the module actually wants every tick.

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| x | A |
| z | B |
| Enter | Start |
| Space | Select |
| q / Ctrl-C | Quit |

The bundled ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, released into the public domain.

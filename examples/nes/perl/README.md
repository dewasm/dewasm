# NES (Perl, ANSI terminal)

An interactive frontend for `nes.wasm` (agnes) that renders straight into the terminal.
It needs no window and no GPU.
`build.sh` builds the reactor library (`examples/apps/scripts/nes.sh`, cached).
It converts the library to Perl with dewasm, into `nes_gen.pl`.
That file is gitignored and regenerated on every build.
`main.pl` loads the demo ROM into guest memory and ticks the emulator.
It draws the framebuffer as 24-bit-color half-blocks, reading keys from the terminal in raw mode.
Core modules only: no CPAN installs.
Raw mode goes through `stty` because `Term::ReadKey` is not core.

Unlike the DOOM Perl frontend (`../../doom/perl`), `nes.wasm` has **zero** wasm imports.
There is no console/save-game/clock host surface to implement.
The module has just `_initialize` plus eight exports:

- `allocRom`, `initGame`, `setInput`, `tickGame`
- `screenOffset`, `paletteOffset`, `frameWidth`, `frameHeight`

The controller is also level-triggered: one `setInput(bitmask)` call per tick.
That differs from DOOM's edge-triggered `reportKeyDown`/`reportKeyUp` pair.
So there's no save-game directory and no per-frame host callback to receive the pixels.
Instead, the host pulls the frame straight out of guest memory after each tick.
It is in the emulator's own representation: one palette *index* per pixel at `screenOffset()`.
Each index resolves against the fixed 64-entry palette at `paletteOffset()` (masked with `0x3f`).
At terminal resolution that means only the sampled pixels are ever looked up.

The bundled ROM is *Alter Ego* by Shiru, released into the public domain.

## Run

```sh
./run.sh
```

takes over the terminal (alternate screen, hidden cursor, raw input) and starts rendering.
`./run.sh --smoke` instead runs a headless self-check.
It loads the ROM, inits the game, and ticks it 40 times with no terminal takeover.
That matches the deterministic driving contract in `crates/dewasm-test-helper/src/nes.rs`.
It sanity-checks the last frame, writes it to `screenshot.ppm`, and exits non-zero on failure.
The file is binary P6, since Perl's core modules include no PNG encoder.
An optional ROM path can be given as the first non-flag argument in either mode.
It defaults to `examples/apps/cache/alter_ego.nes`.

## Honest performance

**Measured ~0.87 ticks/sec, headless, on an Apple Silicon laptop.**
That is a bit faster than DOOM's Perl frontend's ~0.7, but not playable.
`--smoke`'s 40-tick frame is byte-identical to the pinned oracle snapshot.
The snapshot is `examples/apps/snapshots/nes_frame.ppm`.
This confirms the driving contract matches wasmtime's exactly.

## Rendering

Each cell renders two vertically-stacked pixels as a half-block character with 24-bit truecolor SGR.
The NES's native 256x240 framebuffer is downsampled to fit the terminal, capped at 256 columns.
There is no upscaling, unlike DOOM's 2x.
Unchanged cells and repeated SGR codes are skipped.
This is the reference pattern shared with the DOOM and Ruby/Python/Bash frontends.

## Controls

- Arrow keys: d-pad
- `x`: A
- `z`: B
- Enter: Start
- Space: Select
- `q` / Ctrl-C: quit

Terminals only report key-down events, never key-up.
`setInput` wants the whole controller state as one bitmask every tick, not DOOM's press/release pair.
So a release is synthesized by dropping a button from the bitmask.
That happens once ~400ms pass without seeing that key again.
Autorepeat keeps extending the deadline.
A single tap always survives to the very next tick, regardless of the window.
The reason is that the bitmask is read right after the keypress is registered.
The window matters less here than in DOOM.
At this tick rate, a terminal's autorepeat resends the held key many times between ticks.

There are no save games, since `nes.wasm` exposes no such surface.
There is also no menu/HUD text, since `onInfoMessage` doesn't exist here either.
The terminal shows just the framebuffer and a one-line status bar.

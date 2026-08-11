# NES (Perl, ANSI terminal)

An interactive frontend for `nes.wasm` (agnes) that renders straight into the terminal: no window, no GPU.
`build.sh` builds the reactor library (`examples/apps/scripts/nes.sh`, cached) and converts it to Perl with dewasm (`nes_gen.pl`, gitignored, regenerated on every build); `main.pl` loads the demo ROM into guest memory, ticks the emulator, and draws the framebuffer as 24-bit-color half-blocks, reading keys from the terminal in raw mode.
Core modules only: no CPAN installs.
Raw mode goes through `stty` because `Term::ReadKey` is not core.

Unlike the DOOM Perl frontend (`../../doom/perl`), `nes.wasm` has **zero** wasm imports: no console/save-game/clock host surface to wire up, just `_initialize` plus eight exports (`allocRom`, `initGame`, `setInput`, `tickGame`, `screenOffset`, `paletteOffset`, `frameWidth`, `frameHeight`).
The controller is also level-triggered (one `setInput(bitmask)` call per tick, not DOOM's edge-triggered `reportKeyDown`/`reportKeyUp` pair), so there's no save-game directory and no per-frame host callback to receive the pixels; the host instead pulls the frame straight out of guest memory after each tick, in the emulator's own representation: one palette *index* per pixel at `screenOffset()`, resolved against the fixed 64-entry palette at `paletteOffset()` (masked with `0x3f`).
At terminal resolution that means only the sampled pixels are ever looked up.

The bundled ROM is *Alter Ego* by Shiru, released into the public domain.

## Run

```sh
./run.sh
```

takes over the terminal (alternate screen, hidden cursor, raw input) and starts rendering.
`./run.sh --smoke` instead runs a headless self-check: it loads the ROM, inits the game, ticks it 40 times with no terminal takeover (matching the deterministic driving contract in `crates/dewasm-test-helper/src/nes.rs`), sanity-checks the last frame, writes it to `screenshot.ppm` (binary P6: Perl's core modules include no PNG encoder), and exits non-zero on failure.
An optional ROM path can be given as the first non-flag argument in either mode; it defaults to `examples/apps/cache/alter_ego.nes`.

## Honest performance

**Measured ~0.87 ticks/sec, headless, on an Apple Silicon laptop**, a bit faster than DOOM's Perl frontend's ~0.7, but not playable.
`--smoke`'s 40-tick frame is byte-identical to the pinned oracle snapshot (`examples/apps/snapshots/nes_frame.ppm`), confirming the driving contract matches wasmtime's exactly.

## Rendering

Each cell renders two vertically-stacked pixels as a half-block character with 24-bit truecolor SGR; the NES's native 256x240 framebuffer (no upscaling, unlike DOOM's 2x) is downsampled to fit the terminal (capped at 256 columns), with unchanged cells and repeated SGR codes skipped, the same reference pattern shared with the DOOM and Ruby/Python/Bash frontends.

## Controls

Arrows move the d-pad, `x` is A, `z` is B, Enter is Start, Space is Select, `q`/Ctrl-C quits.

Terminals only report key-down events, never key-up, and `setInput` wants the whole controller state as one bitmask every tick (not DOOM's press/release pair), so a release is synthesized by dropping a button from the bitmask once ~400ms pass without seeing that key again (autorepeat keeps extending the deadline, and a single tap always survives to the very next tick regardless of the window, since the bitmask is read right after the keypress is registered).
The window matters less here than in DOOM: at this tick rate a terminal's autorepeat resends the held key many times over before the next tick even happens.

There are no save games (`nes.wasm` exposes no such surface) and no menu/HUD text (`onInfoMessage` doesn't exist here either): just the framebuffer and a one-line status bar.

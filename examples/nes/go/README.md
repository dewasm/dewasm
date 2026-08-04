# NES (Go, ebiten)

An interactive frontend for a converted NES emulator. `build.sh` builds `cache/nes.wasm` (agnes, wrapped with a small `allocRom`/`initGame`/`setInput`/`tickGame` export surface plus `screenOffset`/`paletteOffset` for the frame — see `examples/apps/scripts/nes.sh`) and converts it to Go with dewasm (`nes_gen.go`, gitignored, regenerated on every build). Unlike the DOOM frontend, `nes.wasm` has zero host imports, so `main.go` only loads a ROM into the module's memory and drives the game loop with [ebiten](https://github.com/hajimehoshi/ebiten) — no host-import wiring needed.

## Run

```sh
./run.sh
```

builds and opens a window with the bundled demo ROM ([Alter Ego](https://shiru.untergrund.net/software.shtml) by Shiru, public domain). Pass a path to run a different ROM: `./run.sh path/to/game.nes`. `./run.sh -smoke` instead runs a headless self-check: it inits the game, ticks it 300 times with no window, sanity-checks the last frame, writes it to `screenshot.png`, and exits non-zero on failure.

The game loop runs at ebiten's default 60 TPS, which is close enough to the NTSC NES's native ~60.0988 Hz that no explicit `SetTPS` override is needed (unlike DOOM's 35 Hz tic rate).

## Controls

- Arrow keys: D-pad
- X: A
- Z: B
- Enter: Start
- Space: Select
- Escape / window close: quit

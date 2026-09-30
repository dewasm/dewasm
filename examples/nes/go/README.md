# NES (Go, Ebitengine)

An interactive frontend for a converted NES emulator.
`build.sh` builds `cache/nes.wasm`, which is `agnes` wrapped with a small export surface.
The exports are `allocRom`/`initGame`/`setInput`/`tickGame`, plus `screenOffset`/`paletteOffset` for the frame.
See `examples/apps/scripts/nes.sh`.
`build.sh` converts the module to Go with dewasm, into `nes/nes_gen.go`.
That file is ignored by Git and regenerated on every build.
Unlike the DOOM frontend, `nes.wasm` has zero host imports.
So the frontend, `nes/host.go`, only loads a ROM into the module's memory.
It drives the game loop with [Ebitengine](https://github.com/hajimehoshi/ebiten).
No host imports need implementing.

dewasm converts a module to a Go *package* named after `--module-name`.
So the generated file declares `package nes` and lives in `nes/`.
The frontend reads the module's linear memory directly.
That memory is unexported in Go, so the frontend sits in the same directory.
The command at the directory top level is two lines: import the package, call `nes.Run()`.
An embedder that only calls exports needs none of this: it can import the package from anywhere.

## Run

```sh
./run.sh
```

builds and opens a window with the bundled demonstration ROM.
That ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, public domain.
Pass a path to run a different ROM: `./run.sh path/to/game.nes`.
`./run.sh -smoke` instead runs a headless self-check.
It initializes the game and ticks it 300 times with no window.
It sanity-checks the last frame, writes it to `screenshot.png`, and exits non-zero on failure.

The game loop runs at Ebitengine's default 60 TPS.
That is close enough to the NTSC NES's native ~60.0988 Hz to need no explicit `SetTPS` override.
DOOM's rate of 35 ticks per second, by contrast, does need one.

## Controls

- Arrow keys: D-pad
- X: A
- Z: B
- Enter: Start
- Space: Select
- Escape / window close: exit

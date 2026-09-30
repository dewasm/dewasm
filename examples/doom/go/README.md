# DOOM (Go, Ebitengine)

An interactive frontend for the DOOM shareware episode, running as pure Go code.
`build.sh` fetches `jacobenget/doom.wasm`, checked against a fixed checksum, into the shared apps cache.
It converts the module to Go with dewasm, into `doom/doom_gen.go`.
That file is ~10MB, ignored by Git, and regenerated on every build.
`build.sh` then links it against a small host program in `doom/host.go`.
The host program implements the module's ten host imports.
They cover console messages, save-game files, the game clock, and frame delivery.
The host program also drives the game loop with [Ebitengine](https://github.com/hajimehoshi/ebiten).

dewasm converts a module to a Go *package* named after `--module-name`.
So the generated file declares `package doom` and lives in `doom/`.
The frontend reads the module's linear memory and exported globals directly.
Those are unexported in Go, so the frontend sits in the same directory.
The command at the repository top level is two lines: import the package, call `doom.Run()`.
An embedder that only calls exports needs none of this: it can import the package from anywhere.

## Run

```sh
./run.sh
```

builds and opens a window.
`./run.sh -smoke` instead runs a headless self-check.
It initializes the game and ticks it 300 times with no window.
It sanity-checks the last frame, writes it to `screenshot.png`, and exits non-zero on failure.

The game loop runs at 35 ticks per second, DOOM's own internal tic rate.
So every `Update()` call advances exactly one game tic instead of some calls doing nothing.
The module paces itself internally off a monotonic clock, regardless of how often it's ticked.

## Controls

- Arrow keys: move / turn
- Ctrl: fire
- Space: use / open doors
- Shift: run
- Comma / period: strafe left / right
- Tab: automap
- Escape / Enter / Backspace: menus
- Letters and digits: text entry and prompts (`y` confirms, digits select weapons)

Save games are written to `.savegame/` (ignored by Git) relative to wherever the binary runs.

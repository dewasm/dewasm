# DOOM (Go, ebiten)

An interactive frontend for the DOOM shareware episode, running as pure Go code. `../fetch.sh` downloads jacobenget/doom.wasm; `build.sh` converts it to Go with dewasm (`doom_gen.go`, ~10MB, gitignored, regenerated on every build) and links it against a small host program in `main.go` that implements the module's ten host imports (console messages, save-game files, the game clock, and frame delivery) and drives the game loop with [ebiten](https://github.com/hajimehoshi/ebiten).

## Run

```sh
./run.sh
```

builds and opens a window. `./run.sh -smoke` instead runs a headless self-check: it inits the game, ticks it 300 times with no window, sanity-checks the last frame, writes it to `screenshot.png`, and exits non-zero on failure.

The game loop runs at 35 ticks per second — DOOM's own internal tic rate — so every `Update()` call advances exactly one game tic instead of some calls being no-ops (the module paces itself internally off a monotonic clock regardless of how often it's ticked).

## Controls

- Arrow keys: move / turn
- Ctrl: fire
- Space: use / open doors
- Shift: run
- Comma / period: strafe left / right
- Tab: automap
- Escape / Enter / Backspace: menus
- Letters and digits: text entry and prompts (`y` confirms, digits select weapons)

Save games are written to `.savegame/` (gitignored) relative to wherever the binary runs.

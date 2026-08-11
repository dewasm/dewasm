# DOOM (Java, Swing)

An interactive DOOM frontend built on a pure-Java library that dewasm generated from [jacobenget/doom.wasm](https://github.com/jacobenget/doom.wasm) (the shareware WAD is embedded in the wasm module, so no game data files are needed).
`Main.java` implements the module's tiny host interface (console logging, save-game I/O, timing, and the framebuffer blit) with a Swing window: a `BufferedImage` is filled from wasm linear memory each frame and drawn scaled into the window, and keyboard input is queued from a `KeyListener` and drained on the dedicated game thread that ticks DOOM.

Zero external dependencies: only the JDK (`javac`/`java`, AWT/Swing, NIO).

## Run

```sh
./run.sh
```

This fetches/builds the wasm module, regenerates the Java library with dewasm, compiles, and launches the window.
`./build.sh` alone does the fetch/generate/compile without launching.

`java -cp classes Main --smoke` runs a headless self-test (no window): it ticks the game, writes the final frame to `screenshot.png`, and prints measured ticks/sec.

## Controls

| Key | Action |
| --- | --- |
| Arrow keys | Move / turn |
| Ctrl | Fire |
| Space | Use (open doors, flip switches) |
| Shift | Run |
| , / . | Strafe left / right |
| Tab | Automap |
| Enter | Menu confirm |
| Escape | Menu / pause |
| 1-7 | Select weapon |
| Y / N | Confirm prompts |

Save games are written under `.savegame/` (relative to the working directory).

# DOOM (Java, Swing)

An interactive DOOM frontend built on a library written only in Java.
dewasm generated that library from [`jacobenget/doom.wasm`](https://github.com/jacobenget/doom.wasm).
The shareware WAD is embedded in the wasm module, so no game data files are needed.
`Main.java` implements the module's tiny host interface with a Swing window.
The interface covers console logging, save-game I/O, timing, and the framebuffer copy.
Each frame, a `BufferedImage` is filled from wasm linear memory and drawn scaled into the window.
Key input is collected from a `KeyListener`.
The dedicated game thread that ticks DOOM drains it.

Zero external dependencies: only the JDK (`javac`/`java`, AWT/Swing, NIO).

## Run

```sh
./run.sh
```

This fetches and builds the wasm module and regenerates the Java library with dewasm.
It then compiles and launches the window.
`./build.sh` alone does the fetch/generate/compile without launching.

`java -cp classes Main --smoke` runs a headless self-test, with no window.
It ticks the game, writes the final frame to `screenshot.png`, and prints measured ticks/sec.

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

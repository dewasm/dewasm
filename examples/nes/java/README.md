# NES (Java, Swing)

An interactive NES frontend built on a pure-Java library that dewasm generated from `nes.wasm`, an import-free reactor (`examples/apps/scripts/nes.sh`, `examples/apps/src/nes_demo.c`) wrapping [kgabis/agnes](https://github.com/kgabis/agnes), built from source with zig.
`Main.java` reads the ROM, copies it into the module's linear memory via `allocRom`, then drives `setInput`/`tickGame` and composes each frame into a `BufferedImage` straight out of guest memory: one palette *index* per pixel at `screenOffset()` against the fixed palette at `paletteOffset()`, decoded once into ARGB ints.
The module has no host callbacks at all (unlike DOOM's console/save/UI import surface), so the frontend just pulls state after every tick, paced to 60 Hz on a dedicated game thread.

Zero external dependencies: only the JDK (`javac`/`java`, AWT/Swing, NIO).

The default ROM is [Alter Ego](https://shiru.untergrund.net/nesdev.shtml) by Shiru, released into the public domain, fetched and pinned by `examples/apps/scripts/nes.sh`.

## Run

```sh
./run.sh
```

This fetches/builds the wasm module, regenerates the Java library with dewasm, compiles, and launches the window.
`./build.sh` alone does the fetch/generate/compile without launching.
Pass a `.nes` file path to run a different ROM: `./run.sh path/to/game.nes`.

`java -cp classes Main --smoke` runs a headless self-test (no window): it ticks the game, writes the final frame to `screenshot.png`, and prints measured ticks/sec.

## Controls

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| X | A |
| Z | B |
| Enter | Start |
| Space | Select |
| Escape / close window | Quit |

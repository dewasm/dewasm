# NES (Java, Swing)

An interactive NES frontend built on a library written only in Java.
dewasm generated that library from `nes.wasm`.
`nes.wasm` is an import-free reactor wrapping [`kgabis/agnes`](https://github.com/kgabis/agnes), built from source with `wasi-sdk`.
Its sources are `examples/apps/scripts/nes.sh` and `examples/apps/src/nes_demo.c`.
`Main.java` reads the ROM and copies it into the module's linear memory via `allocRom`.
It then drives `setInput`/`tickGame`.
It composes each frame into a `BufferedImage` straight out of guest memory.
The frame is one palette *index* per pixel at `screenOffset()`.
The indices resolve against the fixed palette at `paletteOffset()`, decoded once into ARGB integers.
The module has no host callbacks at all, unlike DOOM's console/save/UI import surface.
So the frontend just pulls state after every tick, paced to 60 Hz on a dedicated game thread.

Zero external dependencies: only the JDK (`javac`/`java`, AWT/Swing, NIO).

The default ROM is [Alter Ego](https://shiru.untergrund.net/nesdev.shtml) by Shiru, released into the public domain.
`examples/apps/scripts/nes.sh` fetches it and checks it against a fixed hash.

## Run

```sh
./run.sh
```

This fetches and builds the wasm module and regenerates the Java library with dewasm.
It then compiles and launches the window.
`./build.sh` alone does the fetch/generate/compile without launching.
Pass a `.nes` file path to run a different ROM: `./run.sh path/to/game.nes`.

`java -cp classes Main --smoke` runs a headless self-test, with no window.
It ticks the game, writes the final frame to `screenshot.png`, and prints measured ticks/sec.

## Controls

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| X | A |
| Z | B |
| Enter | Start |
| Space | Select |
| Escape / close window | Exit |

# DOOM on dewasm

One DOOM, two languages: the unmodified [jacobenget/doom.wasm](https://github.com/jacobenget/doom.wasm) v0.1.0 binary — DOOM compiled to a single wasm module with a ten-function host interface and the shareware WAD embedded — converted by `dewasm --mode library` and played through a native frontend per language:

- [`go/`](go/) — Go, rendering with [ebiten](https://github.com/hajimehoshi/ebiten)
- [`java/`](java/) — Java, rendering with Swing (plain JDK, zero dependencies)

Each frontend implements the same ten imports (framebuffer hand-off, monotonic clock, WAD loading, save games, console logging) in its own language and drives the exported `initGame`/`tickGame`/`reportKeyDown`/`reportKeyUp`. The wasm module is the portable artifact; only the host layer differs ([ADR-50](../../docs/adr/50-doom-example-shape.md)).

## Run

```sh
go/run.sh    # or: java/run.sh
```

`fetch.sh` (invoked by the build scripts) downloads the wasm binary into the gitignored `cache/`; no other assets are needed. Each frontend also has a headless `-smoke`/`--smoke` mode that ticks the game without a window, sanity-checks the rendered frame, and writes it to `screenshot.png`.

Measured on an Apple Silicon laptop, headless: Go ~70 ticks/sec, Java ~55 — both comfortably above DOOM's native 35Hz tic rate. The same converted library also boots under the Ruby (~15 ticks/sec with YJIT) and Python (~1.3 ticks/sec) backends; only a host layer is missing there.

No sound: the module exposes no audio interface. This example is built by its own scripts and is not part of `cargo test`.

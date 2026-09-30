# Decision 59: NES Example (A Self-Built Guest with a File-Based ROM and an Export-Only Interface)

Status: **Accepted, 2026-08-04.**
`examples/nes` runs a NES emulator converted with `--mode library` on all six backends.
It follows the DOOM example's shape ([decision 50](50-doom-example-shape.md)):

- one wasm artifact;
- per-language native frontends;
- a deterministic framebuffer snapshot ([decision 53](53-doom-frame-snapshot.md)/[decision 56](56-unified-snapshot-regeneration.md)).

Unlike DOOM there is no suitable upstream binary, so `nes.wasm` is built from source.
`examples/apps/scripts/nes.sh` builds it from an emulator library plus a thin wrapper.
The library is kept at one fixed version.
The wrapper is checked in at `examples/apps/src/nes_demo.c`.

Revised 2026-08-04: the frame is handed over as `agnes`'s own palette-index buffer plus its palette.
The exports `screenOffset`/`paletteOffset` locate the two.
They replace a BGRA image rendered in-guest, which cost 12-15% of frame time on every backend.
That image told the host nothing it could not compose itself (issue #117).
The composed pixels are identical, so the recorded snapshot is unchanged.

Extended with `ruby/gui/`, a windowed Ruby frontend on `gosu`.
It shares the terminal frontend's generated library.
The reasons for a second frontend and the bundler scoping are recorded in [decision 50](50-doom-example-shape.md).
That decision records them for the DOOM example's matching frontend.

## Context

The DOOM example demonstrates library mode's *import* surface: the host implements ten functions.
Its criterion is worth reusing: the wasm module is the portable artifact.
In that criterion the host side is the only part to port.
But DOOM builds its game data (the shareware WAD) into the guest.
So the DOOM example runs exactly one program.
A console emulator is the natural next specimen: the guest is a fixed machine.
The program is data (a ROM file).
So one artifact can run anything the emulator's mapper coverage admits.
No upstream ships a NES emulator as a plain wasm32 binary with an embeddable interface.
The existing wasm ports are Emscripten/`wasm-bindgen` builds whose import surfaces are JS glue.
dewasm deliberately does not target JS glue.

## Decision

Build the guest ourselves from [`agnes`](https://github.com/kgabis/agnes).
It is a single-pair C NES emulator library under the MIT license, with no dependencies.
It supports the mappers NROM/UxROM/MMC1/MMC3 and has no APU.
It is checked per-file against a fixed sha256, like every locally-built app.
That rule follows [decision 22](22-sqlite3-built-from-source.md) and [decision 39](39-running-wasm-opt.md).
Two interface decisions, each the deciding criterion for future emulator-style examples:

- **The ROM is a separate file, loaded by the host.**
  The frontend reads the `.nes` file and copies it into guest memory through `allocRom(size) -> ptr`.
  The guest never sees a file system.
  Criterion: *data the example exists to swap stays outside the artifact.*
  *Data the example never varies (DOOM's WAD) may stay inside.*
  The example's ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru.
  It is public domain, mapper 0, and fetched with a fixed checksum.
  Any ROM within `agnes`'s mappers works via the frontends' optional path argument.
- **The interface is export-only: the import section is empty.**
  `nes.sh` fails the build if an import appears.
  Exports: `allocRom`, `initGame`, `setInput`, `tickGame`, four frame exports, and `memory`.
  The frame exports are `screenOffset`/`paletteOffset`/`frameWidth`/`frameHeight`.
  DOOM needed a host clock import to end its internal waits.
  A NES frame is a fixed unit of work (1/60 s of console time).
  So pacing belongs wholly to the host, and the guest needs nothing from it.
  Criterion: *an import earns its place only when the guest cannot progress without the host's answer.*
  *Pacing, input polling, and presentation never qualify.*
  The frame crosses the boundary in the emulator's own representation.
  That is one palette index per pixel (masked with `0x3f`) plus the fixed 64-entry `R,G,B,A` palette.
  The host composes the pixels: rendering is presentation.
  A per-pixel loop is also far cheaper in the host language than in converted wasm.
  `nes_demo.c` `#include`s `agnes.c` to reach those internals, so no upstream patch is needed.
  Criterion: *hand over the guest's own representation; convert only in the host.*
  `crates/dewasm-test-helper/src/nes.rs` carries the shared encoder.

The snapshot is captured after 40 input-free frames.
That is the smallest count safely inside the first stable screen.
The credits fade-in completes at ~37.
Every extra frame is real wall time in Bash's `ultra` category.
The blank-frame guard in `crates/xtask/src/nes_snapshot.rs` accepts ≥5 distinct colors.
NES palettes are small (the recorded frame has 7), so DOOM's >50 threshold does not transfer.

## Rejected alternatives

- **Embedding the ROM at build time (DOOM's shape).**
  Makes `nes.wasm` single-program and forces a rebuild per ROM.
  That loses the emulator's whole point as an example.
  It also couples the artifact's recorded hash to the ROM's.
- **Guest-side ROM loading via WASI.**
  Natural in C.
  But it pushes preopen and `argv` set-up into all six frontends and the snapshot oracle.
  That gives up the empty import section for nothing the example would show.
  WASI file I/O is already exercised by CPython, CRuby, and sqlite3.
- **Other emulator cores.**
  `binjnes` (more accurate, more mappers) is tied to its Emscripten/SDL host layers.
  `smolnes` is code-golfed beyond modification; the Rust cores target `wasm-bindgen`.
  Criterion as above: the core must compile to wasm32 with no host assumptions we don't control.

## Consequences

- Positive: first example where the converted artifact is a *platform* rather than a program.
  The frontends are six native NES players.
  The export-only interface is the simplest possible library-mode embedding.
  It is a gentler reference than DOOM's ten imports.
- Negative: `agnes` has no APU (silent, like DOOM), and its mapper coverage caps which ROMs run.
  Its accuracy is that of a spare-time project.
  That is accepted because the example claims an exact conversion, not an accurate emulator.
  An exact conversion here means byte-identical frames across backends.
- Carry-over: `setInput` is untested by the snapshot (input-free by design).
  The frontends are its only exercise.

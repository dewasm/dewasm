# ADR-59 — NES Demo: A Self-Built Guest with a File-Based ROM and an Export-Only Interface

Status: **Accepted, 2026-08-04.** `examples/nes` runs a NES emulator converted with `--mode library` on all six backends, in the DOOM demo's shape ([ADR-50](50-doom-example-shape.md)): one wasm artifact, per-language native frontends, a deterministic framebuffer snapshot ([ADR-53](53-doom-frame-golden.md)/[ADR-56](56-unified-snapshot-regeneration.md)). Unlike DOOM there is no suitable upstream binary, so `examples/apps/scripts/nes.sh` builds `nes.wasm` from a pinned emulation library plus a thin wrapper checked in at `examples/apps/src/nes_demo.c`.

## Context

The DOOM example demonstrates library mode's *import* surface: the host implements ten functions. Its criterion — the wasm module is the portable artifact, the host side is the only porting seam — is worth reusing, but DOOM bakes its game data (the shareware WAD) into the guest, so the demo runs exactly one program. A console emulator is the natural next specimen: the guest is a fixed machine and the program is data (a ROM file), so one artifact can run anything the emulator's mapper coverage admits. No upstream ships a NES emulator as a plain wasm32 binary with an embeddable interface; the existing wasm ports are Emscripten/wasm-bindgen builds whose import surfaces are JS glue, which dewasm deliberately does not target.

## Decision

Build the guest ourselves from [agnes](https://github.com/kgabis/agnes) (single-pair C NES emulation library, MIT, no dependencies, mappers NROM/UxROM/MMC1/MMC3, no APU), pinned per-file by sha256 like every locally-built app ([ADR-22](22-sqlite3-built-from-source.md), [ADR-39](39-wasm-opt-preprocessing.md)). Two interface decisions, each the discriminating criterion for future emulator-style demos:

- **The ROM is a separate file, loaded by the host.** The frontend reads the `.nes` file and copies it into guest memory through `allocRom(size) -> ptr`; the guest never sees a filesystem. Criterion: *data the demo exists to swap stays outside the artifact; data the demo never varies (DOOM's WAD) may stay inside.* The demo ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru (public domain, mapper 0), fetched checksum-pinned; any ROM within agnes's mappers works via the frontends' optional path argument.
- **The interface is export-only — the import section is empty** (`nes.sh` fails the build if an import appears). Exports: `allocRom`, `initGame`, `setInput`, `tickGame`, `frameOffset`/`frameWidth`/`frameHeight`, `memory`. Where DOOM needed a host clock import to terminate its internal waits, a NES frame is a fixed unit of work (1/60 s of console time), so pacing belongs wholly to the host and the guest needs nothing from it. Criterion: *an import earns its place only when the guest cannot progress without the host's answer; pacing, input polling, and presentation never qualify.* The framebuffer is B,G,R,A like DOOM's, so `frame_to_ppm` and the per-backend glue shapes carry over unchanged (`crates/dewasm-test-helper/src/nes.rs`).

The snapshot is captured after 40 input-free frames — the smallest count safely inside the first stable screen (the credits fade-in completes at ~37; every extra frame is real wall time on Bash's ultra tier). The degenerate-frame guard accepts ≥5 distinct colors: NES palettes are small (the pinned frame has 7), so DOOM's >50 threshold does not transfer (`crates/xtask/src/nes_snapshot.rs`).

## Rejected alternatives

- **Embedding the ROM at build time (DOOM's shape).** Makes `nes.wasm` single-program and forces a rebuild per ROM, discarding the emulator's whole point as a demo; also couples the artifact pin to the ROM pin.
- **Guest-side ROM loading via WASI.** Natural in C but pushes preopen/argv wiring into all six frontends and the snapshot oracle, surrendering the empty import section for no demonstrative gain — WASI file I/O is already exercised by CPython, CRuby, and sqlite3.
- **Other emulator cores.** binjnes (more accurate, more mappers) is entangled with its Emscripten/SDL host layers; smolnes is code-golfed beyond modification; the Rust cores target wasm-bindgen. Criterion as above: the core must compile to wasm32 with no host assumptions we don't control.

## Consequences

- Positive: first demo where the converted artifact is a *platform* rather than a program — the frontends are six native NES players. The export-only interface is the simplest possible library-mode embedding, a gentler reference than DOOM's ten imports.
- Negative: agnes has no APU (silent, like DOOM) and its mapper coverage caps which ROMs run; accuracy is hobby-grade, accepted because the demo claims conversion fidelity (byte-identical frames across backends), not emulation fidelity.
- Carry-over: `setInput` is untested by the snapshot (input-free by design); the frontends are its only exercise.

# ADR-50 — DOOM Demo: One Wasm Binary, Per-Language Native Frontends

Status: **Accepted, 2026-07-30.** `examples/doom` runs the unmodified [jacobenget/doom.wasm](https://github.com/jacobenget/doom.wasm) v0.1.0 release binary through `--mode library`, with an ebiten frontend for the Go backend and a Swing frontend for the Java backend; both pass a headless smoke that renders a real frame well above DOOM's native 35Hz tic rate. Extended the same day with Ruby and Python frontends that render into the terminal (ANSI truecolor half-blocks, stdlib only) — the same criterion applied at slower tick rates, where a pixel window would be pointless but a diffed terminal frame costs under 1ms.

## Context

The Rails demo ([ADR-45](45-rails-sqlite3-shim-example.md)) shows a converted *reactor library* driven through an existing API. It has no counterpart for interactive programs where the host owns the event loop, the clock, and the screen — the case where `--mode library`'s import surface, not WASI, is the platform boundary. doom.wasm is the sharpest available specimen: ten imported functions (framebuffer hand-off, monotonic clock, WAD loading, save games, logging), four exports plus memory, no WASI, and the shareware WAD embedded so the demo needs zero game assets.

## Decision

Convert **one upstream release binary, unmodified, once per language**, and implement its ten-function import surface natively in each frontend — Go with ebiten, Java with Swing. Criterion, reusable for future interactive demos: *the wasm module is the portable artifact and the import surface is the porting seam; a frontend may only differ in host-side code, never by rebuilding or patching the guest.* Two consequences of the criterion: frontends stay dependency-light in each language's idiom (Swing is plain JDK; ebiten is the one Go dependency), and every frontend carries a headless `-smoke` mode that ticks the game and PNG-dumps the framebuffer, so the semantics-bearing path (memory layout, pixel format, clock pacing) is verified without a window. The example is documentation-tier: fetched and built by its own scripts, outside the `cargo test` tiers ([ADR-48](48-slow-test-tiers.md)).

## Rejected alternatives

- **Standalone mode over a WASI port of DOOM.** Inverts the demo: WASI has no display/input surface, so the interesting part would live in a bespoke shim, and nothing would exercise `--mode library`'s host-import path — the thing this example exists to show.
- **Per-language guest builds (e.g. Emscripten JS glue, TinyGo-side ports).** Breaks the one-artifact claim that makes the demo persuasive; every frontend would demonstrate a different binary.
- **A uniform SDL binding layer in every language.** One rendering stack to learn, but it imports a C dependency into languages whose selling point here is "plain JDK" / "one idiomatic game library", and SDL bindings for Java are effectively abandonware. Rejected for Java specifically against JavaFX (external module since JDK 11) and libGDX (build-system weight disproportionate to a framebuffer blit).

## Consequences

- Positive: first interactive, real-time proof of the Go and Java backends (measured headless: ~70 and ~55 ticks/sec against DOOM's 35Hz target); a reference embedding for the library-mode import surface in both languages, complementing [ADR-45](45-rails-sqlite3-shim-example.md)'s export-driven shape.
- Negative: the example is unguarded by CI (network fetch, GUI); upstream doom.wasm is pinned to v0.1.0 and interface drift would surface only when someone reruns `build.sh`. No sound — the module exposes no audio interface.
- Carry-over: the Ruby (~15 ticks/sec under YJIT) and Python (~1.3) frontends confirmed the criterion — each was a new host layer only, with the guest untouched. The terminal renderer doubles as the frontend shape for any future backend too slow for a window (Bash being the open question).

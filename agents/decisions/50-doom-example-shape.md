# Decision 50: DOOM Demo (One Wasm Binary, Per-Language Native Frontends)

Status: **Accepted, 2026-07-30.**
`examples/doom` runs the unmodified [jacobenget/doom.wasm](https://github.com/jacobenget/doom.wasm) v0.1.0 release binary.
It runs through `--mode library`.
It has an ebiten frontend for the Go backend and a Swing frontend for the Java backend.
Both pass a headless smoke that renders a real frame well above DOOM's native 35Hz tic rate.
It was extended the same day with Ruby and Python frontends that render into the terminal.
They use ANSI truecolor half-blocks, stdlib only.
The same criterion applied at slower tick rates.
There a pixel window would be pointless, but a diffed terminal frame costs under 1ms.
It was extended again with `ruby/gui/`, a windowed Ruby frontend on gosu.
That frontend shares the terminal frontend's generated library.
With it came the first second frontend for a backend that already had one.

## Context

The Rails demo ([decision 45](45-rails-sqlite3-shim-example.md)) shows a converted *reactor library*.
That library is driven through an existing API.
It has no counterpart for interactive programs.
There the host owns the event loop, the clock, and the screen.
In that case `--mode library`'s import surface, not WASI, is the platform boundary.
doom.wasm is the sharpest available specimen:

- ten imported functions (framebuffer hand-off, monotonic clock, WAD loading, save games, logging);
- four exports plus memory;
- no WASI;
- the shareware WAD embedded, so the demo needs zero game assets.

## Decision

Convert **one upstream release binary, unmodified, once per language**.
Implement its ten-function import surface natively in each frontend.
Go uses ebiten, and Java uses Swing.
The criterion is reusable for future interactive demos.
*The wasm module is the portable artifact, and the import surface is the porting seam.*
*A frontend may only differ in host-side code, never by rebuilding or patching the guest.*
The criterion has two consequences:

- Frontends stay dependency-light in each language's idiom.
  Swing is plain JDK; ebiten is the one Go dependency.
- Every frontend carries a headless `-smoke` mode that ticks the game and PNG-dumps the framebuffer.
  So the semantics-bearing path is verified without a window.
  That path is memory layout, pixel format, and clock pacing.

The example is documentation-level: it is fetched and built by its own scripts.
It is outside the `cargo test` speed categories ([decision 48](48-slow-test-speeds.md)).

## Rejected alternatives

- **Standalone mode over a WASI port of DOOM.**
  It inverts the demo: WASI has no display/input surface.
  So the interesting part would live in a bespoke shim.
  Nothing would exercise `--mode library`'s host-import path, the thing this example exists to show.
- **Per-language guest builds (e.g. Emscripten JS glue, TinyGo-side ports).**
  It breaks the one-artifact claim that makes the demo persuasive.
  Every frontend would demonstrate a different binary.
- **A uniform SDL binding layer in every language.**
  One rendering stack to learn, but it imports a C dependency into every language.
  Here those languages' selling point is "plain JDK" / "one idiomatic game library".
  SDL bindings for Java are also effectively abandonware.
  For Java specifically, two more options were rejected:
  - JavaFX, an external module since JDK 11;
  - libGDX, whose build-system weight is disproportionate to a framebuffer blit.

## Consequences

- Positive: first interactive, real-time proof of the Go and Java backends.
  Measured headless: ~70 and ~55 ticks/sec against DOOM's 35Hz target.
  It is a reference embedding for the library-mode import surface in both languages.
  It complements [decision 45](45-rails-sqlite3-shim-example.md)'s export-driven shape.
- Negative: the example is unguarded by CI (network fetch, GUI).
  Upstream doom.wasm is pinned to v0.1.0.
  Interface drift would surface only when someone reruns `build.sh`.
  No sound: the module exposes no audio interface.
  [Decision 53](53-doom-frame-snapshot.md) later closes part of this gap with a deterministic framebuffer snapshot.
  That snapshot is ultra-slow execution plus a CI-side conversion smoke.
- Carry-over: the Ruby (~15 ticks/sec under YJIT) and Python (~1.3) frontends confirmed the criterion.
  Each was a new host layer only, with the guest untouched.

  So did `ruby/gui/`.
  It additionally shows the criterion does not cap a backend at one frontend.
  The title's "per-language" describes how the demo grew, not a limit on it.
  A second frontend for one backend costs nothing beyond its own host code.
  The reason is that it requires the sibling's generated `doom_gen.rb` instead of regenerating its own.

  It also revises the reading that a slow backend implies a terminal.
  What made a window look unaffordable for Ruby was per-pixel cost against a ~15 ticks/sec budget.
  The module's 640x400 framebuffer is an exact 2x upscale of DOOM's native 320x200.
  The log confirms it: `I_InitGraphics: Auto-scaling factor: 2`.
  So halving it back is lossless and leaves a quarter of the pixels to convert.
  The frame is then uploaded once as a 320x200 texture and scaled by the GPU.
  That makes the render cost independent of window size.

  The window also restores real key releases, which no terminal can deliver.
  So Ctrl (fire) and Shift (run) work as in DOOM instead of being synthesized or dropped.
  Its gosu dependency is scoped to `gui/` with bundler.
  `Gemfile.lock` is checked in, and it installs into a gitignored `vendor/bundle`.
  So `ruby/` itself stays stdlib-only.
  The gem version also stays pinned instead of depending on a globally installed gosu.

  The terminal renderer doubles as the frontend shape for any backend too slow for a window.
  That includes Bash, after [decision 51](51-bash-assoc-memory.md)/[decision 52](52-bash-inline-memops.md).
  In `bash/` it boots in ~2 minutes and renders a frame every ~34 seconds.

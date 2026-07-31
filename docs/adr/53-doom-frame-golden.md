# ADR-53 — Test DOOM by a Deterministic Framebuffer Golden

Status: **Accepted, 2026-07-31.** Extends the DOOM demo (ADR-50) into the test
gate: drive the converted `doom.wasm` with a self-advancing synthetic clock and
a fixed, input-free tick count, then compare the raw framebuffer it renders —
pixel for pixel — against a golden captured once from a wasmtime oracle.
Implemented: the oracle (`cargo xtask update-doom-golden`), the shared contract
and runners (`crates/dewasm-test-helper/src/doom.rs`), the committed golden
(`examples/doom/golden/frame.ppm`), and the frame case on Ruby/Python/Go/Java
plus the frame case on all five backends (byte-identical) — Bash at the
ultra_slow tier, the others at slow.

## Context

`examples/doom` converts the unmodified [jacobenget/doom.wasm](https://github.com/jacobenget/doom.wasm)
v0.1.0 to every backend and runs it behind per-language frontends (ADR-50), but
nothing in `cargo test` exercises it. DOOM is the largest real module we
convert and the only one driven purely through **custom host imports** (ten
functions across `console`/`gameSaving`/`runtimeControl`/`ui`/`loading`) with
**no WASI surface at all** — a coverage shape none of the `apps`/`apps_capi`
cases reach. A regression that only this module would surface (a data-segment,
`call_indirect`-table, or accumulated-arithmetic bug in a 640×400 software
renderer) currently ships unnoticed.

Two facts make a golden possible where the module first looks untestable:

- **The framebuffer is a deterministic function of the tick schedule.** DOOM's
  software renderer is fixed-point integer — no floats — so the pixels are pure
  integer computation, identical across wasmtime and every backend regardless of
  the softfloat/NaN conventions (ADR-2/13). The only nondeterminism is the game
  clock: `runtimeControl.timeInMilliseconds` paces the tic loop, and all five
  frontends feed it a *wall* clock (`examples/doom/go/main.go:94`,
  `examples/doom/ruby/main.rb:73`). Override that import with a synthetic
  counter that self-advances a fixed step on every read and drive `initGame`
  then N× `tickGame` with no key events, and the rendered frame is reproducible.
- **`ui.drawFrame(bufOff)` hands the host an offset into linear memory** where
  `FRAME_W*FRAME_H*4` bytes live in `B,G,R,A` order, the alpha byte padding
  (`examples/doom/go/main.go:101-113`). For this pinned binary FRAME is 640×400,
  so the frame is 1,024,000 bytes; dropping the don't-care alpha yields a
  640×400 RGB image — a P6 PPM, the exact format the frontends' own screenshot
  writers already emit (`examples/doom/ruby/main.rb:339-354`).

The existing golden machinery does not stretch to cover this: the wasmtime
ground truth runs through the **`wasmtime` CLI** (`crates/dewasm-test-helper/src/wasmtime_backend.rs:61`),
and `wasmtime run` cannot supply DOOM's custom imports. Producing the oracle
frame needs a real embedder, not the CLI.

## Decision

Add a **deterministic framebuffer golden** test, structured like the C-API cases
(`crates/dewasm-test-helper/src/apps_capi.rs`) — convert `doom.wasm` in library
mode, append per-backend glue, compare output — with three specifics:

- **Driving contract (identical in the oracle and every backend).** Provide the
  ten imports; `timeInMilliseconds` is a counter that self-advances a *large*
  fixed step (1000 ms) on every read. Self-advancing (not frozen between host
  steps) is what stops it hanging — DOOM's startup and inter-tic waits spin on
  the clock, so it must keep moving — and the read count, hence the exact clock
  sequence, is a pure function of the wasm, identical across the oracle and every
  backend. The step is *large* so DOOM's spiral-of-death protection caps the tics
  it simulates (a big jump makes it skip ahead, exactly as the real wall clock
  does when it leaps between a slow backend's calls); a 1 ms step would creep to
  seconds of simulated time and make DOOM run ~80 tics — byte-identical but tens
  of times more work, turning the Bash run into ~an hour. `wadSizes`/`readWads`
  are no-ops
  (the module falls back to its embedded shareware WAD when the out-params stay
  zero, `examples/doom/go/main.go:116-124`), `gameSaving.*` are `0/0/len` no-ops
  (no filesystem, as bash already proves at `examples/doom/bash/main.sh:89-96`).
  Call `initGame`, then N× `tickGame` with no input, and dump the last `drawFrame`
  buffer as a 640×400 P6 PPM (alpha dropped). N is fixed and pinned by the golden
  — the smallest count that clears the demo intro to a stable, non-degenerate
  frame.
- **Oracle = the wasmtime crate, in a golden generator, not the test path.** A
  small Rust host embeds `wasmtime`, runs the *original* `doom.wasm` under the
  driving contract above, and writes `examples/doom/golden/frame.ppm`, refreshed
  on demand via `cargo xtask update-doom-golden` (mirroring
  `update-repl-golden`). The committed PPM is the oracle the per-backend tests
  compare against; the heavy `wasmtime` crate is an **xtask/tooling dependency
  only**, never pulled into the normal `cargo test` build.
- **Fetch as a shared fixture.** `doom.wasm` moves into the apps cache via a new
  `examples/apps/scripts/doom.sh` (checksum-pinned through `fetch_app`),
  replacing the unchecksummed `examples/doom/fetch.sh`; the demo build scripts
  read it from `examples/apps/cache/` like the harness does.

**Discriminating criterion:** *a module driven by custom host imports is still
golden-testable whenever its output is a deterministic function of an injectable
clock — pin the clock, fix the inputs, and diff the rendered artifact.* The
oracle is an independent embedder (wasmtime), not backend consensus, because the
value of the test is catching a bug the backends could share.

**Tiering.** The frame-golden test runs on every backend, tiered to match each
backend's convention for a comparably heavy execution case. Ruby/Python/Go/Java
use the **slow_test** tier — CI's main sweep, like the qjs/sqlite e2e — so DOOM
is actually exercised in CI. Bash uses the **ultra_slow_test** tier (ADR-48),
like its qjs-REPL pty case: its run is minutes (initGame ~2 min + ticks +
serializing a 1 MB framebuffer out of the associative-array memory), so it stays
out of CI and runs only in local pre-release. There is no separate conversion
smoke — the frame test already exercises the full convert-and-run path, and a
convert-only assertion
would be an idiom no other suite uses. *(Amended by
[ADR-54](54-apps-convert-suite.md): the convert-only assertion is now the idiom
of a whole-cache per-backend convert suite, which includes a fast-tier `doom`
convert trial on every backend — the frame-golden run above stays the
convert-and-run gate; the two are complementary.)*

## Rejected alternatives

- **Backend-consensus golden (no external oracle).** Cheaper — no `wasmtime`
  crate — but it only proves the backends *agree*, so a bug in the shared
  converter or the shared numeric conventions (ADR-2) passes. An independent
  embedder is the whole point of a golden; rejected.
- **Sanity-only check (reuse the frontends' `--smoke`).** The frontends already
  assert "enough distinct colors / has glyphs" (`examples/doom/bash/main.sh:400-417`),
  which is cheap but catches only gross breakage and is non-deterministic (wall
  clock). It stays as the demo's self-check; it is not the gate.
- **Oracle via the `wasmtime` CLI, like the apps goldens.** `wasmtime run`
  cannot provide DOOM's custom imports (`crates/dewasm-test-helper/src/wasmtime_backend.rs`);
  unusable here.
- **Extend the frontends with a golden-dump mode instead of dedicated test
  glue.** Avoids duplicating the import wiring, but loads test-only concerns
  (synthetic clock, raw dump) into five demo programs and couples the gate to
  the demo. Dedicated glue consts (the `apps_capi` precedent) keep the two
  separate; the wiring duplication is accepted.

## Consequences

- Positive: the converter gains regression coverage on its largest, most
  import-heavy real module, pinned to a pixel-exact frame an independent runtime
  produced — the strongest oracle available for it.
- Positive: `doom.wasm` becomes a checksum-pinned fixture like every other app
  (closing the ADR-9 gap the old `examples/doom/fetch.sh` left open).
- Negative: a new heavy `wasmtime`-crate dependency, quarantined to xtask; the
  golden must be regenerated (and reviewed) whenever the `doom.wasm` pin bumps.
- Negative / carry-over: the frame test runs in CI for four backends but Bash's
  is ultra_slow (local pre-release only). The synthetic-clock tick count N is a
  magic constant the golden pins — documented at the glue, not derived.

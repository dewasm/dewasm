# Decision 53: Test DOOM by a Deterministic Framebuffer Snapshot

Status: **Accepted, 2026-07-31.**
Extends the DOOM example (decision 50) into a test.
Drive the converted `doom.wasm` with a self-advancing synthetic clock and a fixed tick count.
The tick count is input-free.
Then compare the raw framebuffer it renders, pixel for pixel, against a snapshot.
The snapshot is captured once from a Wasmtime oracle.
Implemented:
- the oracle (`cargo xtask update-doom-snapshot`);
- the shared contract and runners (`crates/dewasm-test-helper/src/doom.rs`);
- the committed snapshot (`examples/doom/snapshot/frame.ppm`);
- the frame case on Ruby/Python/Go/Java plus the frame case on all five backends (byte-identical).

Bash runs at the `ultra` category, the others at `slow`.

## Context

`examples/doom` converts the unmodified [`jacobenget/doom.wasm`](https://github.com/jacobenget/doom.wasm) v0.1.0 to every backend.
It runs it behind per-language frontends (decision 50), but nothing in `cargo test` exercises it.
DOOM is the largest real module we convert.
It is the only one driven purely through **custom host imports** with **no WASI surface at all**.
Those are ten functions across `console`/`gameSaving`/`runtimeControl`/`ui`/`loading`.
This is a coverage shape none of the `apps`/`apps_capi` cases reach.
A regression that only this module would surface currently ships unnoticed.
An example is a data-segment, `call_indirect`-table, or accumulated-arithmetic bug.
The bug would sit in a 640×400 software renderer.

Two facts make a snapshot possible where the module first looks untestable:

- **The framebuffer is a deterministic function of the tick schedule.**
  DOOM's software renderer is fixed-point integer (no floats).
  So the pixels are pure integer computation.
  They are identical across Wasmtime and every backend.
  This holds regardless of the softfloat/NaN conventions (decision 2/13).
  The only source of variation is the game clock.
  `runtimeControl.timeInMilliseconds` paces the tic loop.
  All five frontends feed it a *wall* clock.
  Two instances are `examples/doom/go/doom/host.go:100-105` and `examples/doom/ruby/main.rb:73`.
  Override that import with a synthetic counter that self-advances a fixed step on every read.
  Drive `initGame` then N× `tickGame` with no key events, and the rendered frame is the same on every run.
- **`ui.drawFrame(bufOff)` hands the host an offset into linear memory.**
  There `FRAME_W*FRAME_H*4` bytes live in `B,G,R,A` order, the `A` byte unused.
  The layout is read at `examples/doom/go/doom/host.go:107-122`.
  For the binary at this checksum FRAME is 640×400, so the frame is 1,024,000 bytes.
  Dropping the unused `A` byte yields a 640×400 RGB image, a P6 PPM.
  That is the exact format the frontends' own writers of screen images already emit.
  One writer is at `examples/doom/ruby/main.rb:339-354`.

The existing snapshot machinery does not stretch to cover this.
The Wasmtime ground truth runs through the **`wasmtime` CLI**.
That call is at `crates/dewasm-test-helper/src/wasmtime_backend.rs:61`.
`wasmtime run` cannot supply DOOM's custom imports.
Producing the oracle frame needs a real embedder, not the CLI.

## Decision

Add a **deterministic framebuffer snapshot** test, structured like the C-API cases.
Those are in `crates/dewasm-test-helper/src/apps_capi.rs`.
Convert `doom.wasm` in library mode, add per-backend glue, compare output.
Three specifics:

- **Driving contract (identical in the oracle and every backend).**
  Provide the ten imports.
  `timeInMilliseconds` is a counter that self-advances a *large* fixed step (one second) on every read.
  Self-advancing (not frozen between host steps) is what stops it hanging.
  DOOM's waits at start and between tics spin on the clock, so it must keep moving.
  The read count, hence the exact clock sequence, is a pure function of the wasm.
  It is identical across the oracle and every backend.
  The step is *large*, so DOOM's protection against falling behind caps the tics it simulates.
  A big jump makes it skip ahead.
  The real wall clock does exactly that when it leaps between a slow backend's calls.
  A 1 ms step would add up to seconds of simulated time.
  DOOM would then run ~80 tics.
  The result is byte-identical but tens of times more work.
  It would turn the Bash run into ~an hour.
  `wadSizes`/`readWads` do nothing.
  The module falls back to its embedded shareware WAD when the output parameters stay zero.
  That fallback is handled at `examples/doom/go/doom/host.go:124-133`.
  `gameSaving.*` return `0/0/len` and do nothing else (no file system).
  Bash already proves that works at `examples/doom/bash/main.sh:89-96`.
  Call `initGame`, then N× `tickGame` with no input.
  Write the last `drawFrame` buffer as a 640×400 P6 PPM (`A` byte dropped).
  N is fixed, and the snapshot is valid only for that N.
  It is the fewest tics that clear the opening demo to a stable frame with real content.
- **Oracle = the `wasmtime` crate, in a snapshot writer, not the test path.**
  A small Rust host embeds `wasmtime` and runs the *original* `doom.wasm` under the driving contract above.
  It writes `examples/doom/snapshot/frame.ppm`.
  The file is refreshed on demand via `cargo xtask update-doom-snapshot`.
  That command mirrors `update-repl-snapshot`.
  The committed PPM is the oracle the per-backend tests compare against.
  The heavy `wasmtime` crate is an **`xtask`/tooling dependency only**.
  It is never pulled into the normal `cargo test` build.
- **Fetch as a shared fixture.**
  `doom.wasm` moves into the apps cache via a new `examples/apps/scripts/doom.sh`.
  `fetch_app` checks it against a fixed checksum.
  That replaces `examples/doom/fetch.sh`, which checked no checksum.
  The example build scripts read it from `examples/apps/cache/` like the harness does.

**Deciding criterion:** *a module driven by custom host imports can still be snapshot-tested.*
*It can when its output is a deterministic function of an injectable clock.*
*Then hold the clock and the inputs constant, and diff the rendered artifact.*
The oracle is an independent embedder (Wasmtime), not backend consensus.
The value of the test is catching a bug the backends could share.

**Speed assignment.**
The frame-snapshot test runs on every backend.
Each backend classifies it to match its convention for a comparably heavy execution case.
Ruby/Python/Go/Java use **`slow_test`** (CI's main run, like the `qjs`/SQLite e2e).
So DOOM is actually exercised in CI.
Bash uses **`ultra_slow_test`** (decision 48), like its `qjs` REPL case under `pty`.
Its run takes minutes: `initGame` ~2 minutes + ticks + writing out a 1 MB framebuffer.
The framebuffer is read out of the associative-array memory.
So the Bash run stays out of CI and runs only in local pre-release.
There is no separate conversion smoke.
The frame test already exercises the full convert-and-run path.
A convert-only assertion would be a pattern no other suite uses.
*(Changed by [decision 54](54-apps-convert-suite.md): the convert-only assertion is now the pattern of a convert suite.*
*That suite is whole-cache and per-backend, and includes a fast `doom` convert trial on every backend.*
*The frame-snapshot run above stays the convert-and-run test; the two cover different things.)*

## Rejected alternatives

- **Backend-consensus snapshot (no external oracle).**
  Cheaper (no `wasmtime` crate), but it only proves the backends *agree*.
  A bug in the shared converter or the shared numeric conventions (decision 2) then passes.
  An independent embedder is the whole point of a snapshot; rejected.
- **Sanity-only check (reuse the frontends' `--smoke`).**
  The frontends already assert "enough distinct colors / has glyphs".
  The assertion is at `examples/doom/bash/main.sh:400-417`.
  That is cheap but catches only large failures, and is non-deterministic (wall clock).
  It stays as the example's self-check; it is not the test.
- **Oracle via the `wasmtime` CLI, like the apps snapshots.**
  `wasmtime run` cannot provide DOOM's custom imports.
  The CLI runner is `crates/dewasm-test-helper/src/wasmtime_backend.rs`.
  It is unusable here.
- **Extend the frontends with a snapshot-writing mode instead of dedicated test glue.**
  Avoids writing the import handling twice.
  But it loads test-only concerns (synthetic clock, raw frame output) into five example programs.
  It also couples the test to the example.
  Dedicated glue constants (as `apps_capi` does) keep the two separate.
  The import handling written twice is accepted.

## Consequences

- Positive: the converter gains regression coverage on its largest, most import-heavy real module.
  The check compares against a pixel-exact frame an independent runtime produced.
  That is the strongest oracle available for it.
- Positive: `doom.wasm` becomes a fixture checked against a checksum like every other app.
  This closes the decision 9 gap the old `examples/doom/fetch.sh` left open.
- Negative: a new heavy `wasmtime`-crate dependency, kept to `xtask`.
  The snapshot must be regenerated (and reviewed) whenever the `doom.wasm` version changes.
- Negative / carry-over: the frame test runs in CI for four backends.
  Bash's is `ultra` (local pre-release only).
  The synthetic-clock tick count N is a magic constant the snapshot depends on.
  It is documented at the glue, not derived.

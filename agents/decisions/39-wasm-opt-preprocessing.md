# Decision 39: wasm-opt Preprocessing of Locally-Built App Modules

Status: **Accepted, 2026-07-28.**
Implemented in `examples/apps/scripts/*.sh` (via the `wasm_opt_inplace` helper in `common.sh`): `wasm-opt -O2` (baseline features only) runs over every module the script builds from source (the three sqlite3 shapes, minigzip, libpcap, tree-sitter, and ripgrep), with `wasm-opt --version` folded into each module's rebuild stamp.
(The pass initially covered only libpcap/tree-sitter/ripgrep; it was later extended to sqlite3 and minigzip, which the "build it ⇒ eligible" rule below always implied, once their snapshots were re-verified against the optimized output.)

## Context

The example apps split into two kinds ([decision 9](9-example-apps-from-registry.md), [decision 22](22-sqlite3-built-from-source.md)): prebuilt upstream artifacts we only download, and modules we build ourselves from pinned source (the sqlite3 shapes, minigzip, ripgrep, and the Track A pair libpcap and tree-sitter).
The locally-built ones ship as the raw toolchain output, which is bigger than it needs to be and carries two encoding quirks: zig/clang emit DWARF debug info and overlong LEB `call_indirect` immediates (the "reference-types encoding only" artifact the audit tolerates, [decision 8](8-latest-testsuite-support-matrix.md) footnote).
Every extra kilobyte is paid again at conversion time, and the libpcap/tree-sitter modules are converted on every heavy-conditional e2e run.

`wasm-opt -O2` (binaryen) shrinks these substantially (libpcap 2.0 MB → 263 KB, tree-sitter 1.5 MB → 87 KB, ripgrep 22 MB → 18 MB) and, as a side effect, re-encodes the `call_indirect` immediates so the modules audit as *pure* baseline rather than baseline + the reference-types bit.
Only modules we build qualify: a downloaded upstream artifact is pinned by its published checksum and must not be silently rewritten.

## Decision

Run `wasm-opt -O2` in-place over each locally-built module immediately after it is compiled, before it lands in the cache.
The discriminating rule: **preprocess a module only if `setup.sh` builds it from source (and can therefore re-verify it); never a downloaded artifact.**
Concretely this is every built-from-source module (the three sqlite3 shapes, minigzip, libpcap, tree-sitter, and ripgrep), with two exceptions: the DWARF fixture (`dwarf-fixture.sh`) is skipped because its `-g` debug info is the whole point of the case (decision 38), which wasm-opt would strip, and mruby (`mruby.sh`) is skipped because the pinned baseline flag set below cannot parse its exception-handling instructions ([decision 69](69-exception-handling-accepted-input.md)), so that build strips debug info at link time with `-Wl,--strip-debug` instead.

Three constraints on how:

- **Baseline features only.**
  `wasm-opt` is invoked with exactly the universal baseline feature set enabled (`--enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int --enable-mutable-globals --enable-multivalue --enable-reference-types`) and nothing else, so it can neither reject the bulk-memory the toolchain emits nor introduce a construct outside 0.1 scope (SIMD/atomics/exception-handling, [decision 24](24-01-scope-reset.md)).
  The audit is re-run on the output to confirm it stays in scope.
- **No `wasm-ctor-eval.`**
  Only `wasm-opt`; the ctor-evaluator (which partially executes a module's start/ctors at build time) is deliberately not used: it is a heavier, behaviour-altering transform we do not need and would have to re-validate separately.
- **`--strip-debug` at link for the zig builds.**
  `wasm-opt` cannot parse the DWARF zig emits (`Fatal: TODO: DW_LNE_define_file`), so every `zig cc` link (the sqlite3 shapes, minigzip, libpcap, tree-sitter) passes `-Wl,--strip-debug`; ripgrep's rustc release output needs no stripping.

Behaviour preservation is verified, not assumed: after adoption the libpcap / tree-sitter C-API cases and ripgrep's `rg_search` e2e were re-run, and ripgrep's wasmtime snapshot was re-checked via `cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime` (all passing), the extra ground-truth test ripgrep needs because it is the one preprocessed module with a committed wasmtime snapshot.
When the pass was extended to sqlite3 and minigzip, the same re-run confirmed it: the sqlite3 shell/C-API cases and the byte-exact minigzip gz snapshot stayed identical against the optimized binaries.

Each preprocessed module's rebuild stamp is extended from `<source-sha256>` to `<source-sha256>\n<wasm-opt --version>`, compared whole.
A wasm-opt upgrade therefore invalidates the cache and rebuilds the module, so its shipped shape always matches the installed optimizer.
`wasm-opt` joins `zig`/`bison`/`flex` in each affected recipe's fail-loud prerequisite check ([decision 15](15-tests-fail-not-skip.md)).

## Rejected alternatives

- **Optimize every cached module, including downloaded artifacts.**
  Rewriting a checksum-pinned upstream binary breaks the "the cache is exactly the pinned artifact" contract (decision 9) and would make the download's sha256 verification meaningless.
  Downloaded modules are out of scope.
- **Commit the pre-optimized `.wasm`.**
  Third-party artifacts are never committed (decision 9); the optimized output is derived from third-party source and stays in the gitignored cache like everything else `setup.sh` produces.
- **`wasm-ctor-eval` / `-O3` / SIMD-enabled `-O2`.**
  More aggressive transforms buy little for these modules and risk either behaviour changes (ctor-eval) or out-of-scope constructs (SIMD) that the converter would then reject.
  `-O2` with baseline features is the conservative floor that still wins big on size.
- **Skip ripgrep.**
  ripgrep is a shipping app with a committed wasmtime snapshot, so preprocessing it carries snapshot-drift risk.
  It is included only because that snapshot is re-verifiable here (wasmtime installed); the rule stays "build it ⇒ eligible, and re-verify."

## Consequences

- Positive: markedly smaller locally-built modules (faster conversion, less cache footprint) and a cleaner audit verdict (pure baseline, no reference-types encoding bit).
- Positive: the version-stamped cache self-heals on a binaryen upgrade.
- Negative: `wasm-opt` (binaryen) is a new build-time prerequisite for every from-source recipe (sqlite3 and minigzip gained it when the pass was extended to them); absent, they fail loud rather than silently shipping unoptimized modules.
- Carry-over: every from-source module except the DWARF fixture is preprocessed.
  A future locally-built app should adopt the same `wasm_opt_inplace` helper; a future *downloaded* app must not.

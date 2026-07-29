# ADR-39 — wasm-opt Preprocessing of Locally-Built App Modules

Status: **Accepted, 2026-07-28.** Implemented in `examples/apps/fetch.sh`: a `wasm_opt_inplace` helper runs `wasm-opt -O2` (baseline features only) over the three modules the script builds locally — `cache/libpcap.wasm`, `cache/treesitter.wasm`, and `cache/rg.wasm` — with `wasm-opt --version` folded into each module's rebuild stamp.

## Context

The example apps split into two kinds ([ADR-9](9-example-apps-from-registry.md), [ADR-22](22-sqlite3-built-from-source.md)): prebuilt upstream artifacts we only download, and modules we build ourselves from pinned source (the sqlite3 shapes, minigzip, ripgrep, and — Track A — libpcap and tree-sitter). The locally-built ones ship as the raw toolchain output, which is bigger than it needs to be and carries two encoding quirks: zig/clang emit DWARF debug info and overlong LEB `call_indirect` immediates (the "reference-types encoding only" artifact the audit tolerates, [ADR-8](8-latest-testsuite-support-matrix.md) footnote). Every extra kilobyte is paid again at conversion time, and the libpcap/tree-sitter modules are converted on every heavy-gated e2e run.

`wasm-opt -O2` (binaryen) shrinks these substantially — libpcap 2.0 MB → 263 KB, tree-sitter 1.5 MB → 87 KB, ripgrep 22 MB → 18 MB — and, as a side effect, re-encodes the `call_indirect` immediates so the modules audit as *pure* baseline rather than baseline + the reference-types bit. Only modules we build qualify: a downloaded upstream artifact is pinned by its published checksum and must not be silently rewritten.

## Decision

Run `wasm-opt -O2` in-place over each locally-built module immediately after it is compiled, before it lands in the cache. The discriminating rule: **preprocess a module only if `fetch.sh` builds it from source (and can therefore re-verify it); never a downloaded artifact.** Concretely this is libpcap and tree-sitter (the Track A reactor libs) and ripgrep.

Three constraints on how:

- **Baseline features only.** `wasm-opt` is invoked with exactly the universal baseline feature set enabled (`--enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int --enable-mutable-globals --enable-multivalue --enable-reference-types`) and nothing else, so it can neither reject the bulk-memory the toolchain emits nor introduce a construct outside 0.1 scope (SIMD/atomics/exception-handling, [ADR-24](24-01-scope-reset.md)). The audit is re-run on the output to confirm it stays in scope.
- **No `wasm-ctor-eval.`** Only `wasm-opt`; the ctor-evaluator (which partially executes a module's start/ctors at build time) is deliberately not used — it is a heavier, behaviour-altering transform we do not need and would have to re-validate separately.
- **`--strip-debug` at link for the zig builds.** `wasm-opt` cannot parse the DWARF zig emits (`Fatal: TODO: DW_LNE_define_file`), so the two `zig cc` reactor links pass `-Wl,--strip-debug`; ripgrep's rustc release output needs no stripping.

Behaviour preservation is verified, not assumed: after adoption the libpcap / tree-sitter C-API cases and ripgrep's `rg_search` e2e were re-run, and ripgrep's wasmtime golden was re-checked via `cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime` (all green) — the extra ground-truth gate ripgrep needs because it is the one preprocessed module with a committed wasmtime golden.

Each preprocessed module's rebuild stamp is extended from `<source-sha256>` to `<source-sha256>\n<wasm-opt --version>`, compared whole. A wasm-opt upgrade therefore invalidates the cache and rebuilds the module, so its shipped shape always matches the installed optimizer. `wasm-opt` joins `zig`/`bison`/`flex` in each affected recipe's fail-loud prerequisite check ([ADR-15](15-tests-fail-not-skip.md)).

## Rejected alternatives

- **Optimize every cached module, including downloaded artifacts.** Rewriting a checksum-pinned upstream binary breaks the "the cache is exactly the pinned artifact" contract (ADR-9) and would make the download's sha256 verification meaningless. Downloaded modules are out of scope.
- **Commit the pre-optimized `.wasm`.** Third-party artifacts are never committed (ADR-9); the optimized output is derived from third-party source and stays in the gitignored cache like everything else `fetch.sh` produces.
- **`wasm-ctor-eval` / `-O3` / SIMD-enabled `-O2`.** More aggressive transforms buy little for these modules and risk either behaviour changes (ctor-eval) or out-of-scope constructs (SIMD) that the converter would then reject. `-O2` with baseline features is the conservative floor that still wins big on size.
- **Skip ripgrep.** ripgrep is a shipping app with a committed wasmtime golden, so preprocessing it carries golden-drift risk. It is included only because that golden is re-verifiable here (wasmtime installed); the rule stays "build it ⇒ eligible, and re-verify."

## Consequences

- Positive: markedly smaller locally-built modules (faster conversion, less cache footprint) and a cleaner audit verdict (pure baseline, no reference-types encoding bit).
- Positive: the version-stamped cache self-heals on a binaryen upgrade.
- Negative: `wasm-opt` (binaryen) is a new build-time prerequisite for the three affected recipes; absent, they fail loud rather than silently shipping unoptimized modules.
- Carry-over: only these three modules are preprocessed today. A future locally-built app should adopt the same helper; a future *downloaded* app must not.

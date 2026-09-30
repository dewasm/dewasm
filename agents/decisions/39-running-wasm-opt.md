# Decision 39: Running `wasm-opt` on Locally-Built App Modules

Status: **Accepted, 2026-07-28.**
Implemented in `examples/apps/scripts/*.sh`, via the `wasm_opt_inplace` helper in `common.sh`.
`wasm-opt -O2` (baseline features only) runs over every module the script builds from source.
Those are the three sqlite3 shapes, `minigzip`, `libpcap`, tree-sitter, and `ripgrep`.
`wasm-opt --version` is folded into each module's rebuild stamp.
(The pass initially covered only `libpcap`/tree-sitter/`ripgrep`.
It was later extended to sqlite3 and `minigzip`.
The "build it ⇒ eligible" rule below always implied that extension.
The extension came once their snapshots were re-verified against the optimized output.)

## Context

The example apps split into two kinds ([decision 9](9-example-apps-from-registry.md), [decision 22](22-sqlite3-built-from-source.md)):

- prebuilt upstream artifacts, which we only fetch;
- modules we build ourselves from source at a fixed version.
  Those are the sqlite3 shapes, `minigzip`, `ripgrep`, and the Track A pair `libpcap` and tree-sitter.

The locally-built ones ship as the raw toolchain output, which is bigger than it needs to be.
It also carries two unusual encodings from Zig/Clang.
They emit DWARF debug information and overlong LEB `call_indirect` immediates.
The latter is the "reference-types encoding only" artifact the audit accepts.
See the [decision 8](8-latest-testsuite-support-matrix.md) footnote.
Every extra byte is paid again at conversion time.
The `libpcap`/tree-sitter modules are converted on every heavy-conditional e2e run.

`wasm-opt -O2` (Binaryen) reduces these substantially:

| Module | Before | After |
| --- | --- | --- |
| `libpcap` | 2.0 MB | 263 KB |
| tree-sitter | 1.5 MB | 87 KB |
| `ripgrep` | 22 MB | 18 MB |

As a side effect it re-encodes the `call_indirect` immediates.
So the modules audit as *pure* baseline rather than baseline + the reference-types bit.
Only modules we build qualify.
A fetched upstream artifact is checked against its published checksum.
It must not be silently rewritten.

## Decision

Run `wasm-opt -O2` in-place over each locally-built module immediately after it is compiled.
This happens before the module lands in the cache.
The deciding rule: **run `wasm-opt` over a module only if `setup.sh` builds it from source.**
`setup.sh` can then re-verify it; a fetched artifact is never rewritten.
Concretely this is every built-from-source module, with two exceptions.
The modules are the three sqlite3 shapes, `minigzip`, `libpcap`, tree-sitter, and `ripgrep`.
The exceptions are these:

- The DWARF fixture (`dwarf-fixture.sh`) is skipped.
  Its `-g` debug information is the whole point of the case (decision 38), and `wasm-opt` would strip it.
- `mruby` (`mruby.sh`) is skipped.
  The fixed baseline flag set below cannot parse mruby's exception-handling instructions.
  See [decision 69](69-exception-handling-accepted-input.md).
  So that build strips debug information at link time with `-Wl,--strip-debug` instead.

Three constraints on how:

- **Baseline features only.**
  `wasm-opt` is run with exactly the universal baseline feature set enabled and nothing else.
  The set is this:

  ```
  --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int
  --enable-mutable-globals --enable-multivalue --enable-reference-types
  ```

  So `wasm-opt` cannot reject the bulk-memory the toolchain emits.
  It also cannot introduce a construct outside 0.1 scope.
  Out of scope are SIMD/atomics/exception-handling ([decision 24](24-01-scope-reset.md)).
  The audit is re-run on the output to confirm it stays in scope.
- **No `wasm-ctor-eval.`**
  Only `wasm-opt` is used.
  `wasm-ctor-eval` partially executes a module's start function and constructors at build time.
  It is deliberately not used: it is a heavier, behaviour-altering transform we do not need.
  It would also have to be re-validated separately.
- **`--strip-debug` at link for the Zig builds.**
  `wasm-opt` cannot parse the DWARF Zig emits (`Fatal: TODO: DW_LNE_define_file`).
  So every `zig cc` link passes `-Wl,--strip-debug`.
  Those links are the sqlite3 shapes, `minigzip`, `libpcap`, and tree-sitter.
  `ripgrep`'s `rustc` release output needs no stripping.

Behaviour preservation is verified, not assumed.
After adoption, the `libpcap` / tree-sitter C-API cases and `ripgrep`'s `rg_search` e2e were re-run.
`ripgrep`'s Wasmtime snapshot was re-checked too, and all passed.
The command was `cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime`.
`ripgrep` needs that extra ground-truth test.
It is the one module rewritten by `wasm-opt` with a committed Wasmtime snapshot.
When the pass was extended to sqlite3 and `minigzip`, the same re-run confirmed it.
The sqlite3 shell/C-API cases stayed identical against the optimized binaries.
So did the byte-exact `minigzip` `gzip` snapshot.

Each rewritten module's rebuild stamp is extended from `<source-sha256>`.
It becomes `<source-sha256>\n<wasm-opt --version>`, compared whole.
A new `wasm-opt` version therefore makes the cache out of date and rebuilds the module.
So its shipped shape always matches the installed optimizer.
`wasm-opt` joins `zig`/`bison`/`flex` in each affected recipe's fail-loud requirement check.
See [decision 15](15-tests-fail-not-skip.md).

## Rejected alternatives

- **Optimize every cached module, including fetched artifacts.**
  Rewriting an upstream binary checked against a checksum breaks a contract of decision 9.
  The contract is "the cache is exactly the pinned artifact".
  The sha256 verification at fetch time would then check nothing.
  Fetched modules are out of scope.
- **Commit the pre-optimized `.wasm`.**
  Third-party artifacts are never committed (decision 9).
  The optimized output is derived from third-party source.
  It stays in the cache ignored by Git like everything else `setup.sh` produces.
- **`wasm-ctor-eval` / `-O3` / SIMD-enabled `-O2`.**
  More aggressive transforms buy little for these modules.
  They risk either behaviour changes (`wasm-ctor-eval`) or out-of-scope constructs (SIMD).
  The converter would then reject those constructs.
  `-O2` with baseline features is the conservative floor that still wins big on size.
- **Skip `ripgrep`.**
  `ripgrep` is a shipping app with a committed Wasmtime snapshot.
  So rewriting it carries snapshot-drift risk.
  It is included only because that snapshot is re-verifiable here (`wasmtime` installed).
  The rule stays "build it ⇒ eligible, and re-verify."

## Consequences

- Positive: markedly smaller locally-built modules (faster conversion, less cache space).
  The audit verdict is also cleaner: pure baseline, no reference-types encoding bit.
- Positive: the version-stamped cache rebuilds itself on a new Binaryen version.
- Negative: `wasm-opt` (Binaryen) is a new build-time requirement for every from-source recipe.
  sqlite3 and `minigzip` gained it when the pass was extended to them.
  If it is missing, they fail loud rather than silently shipping unoptimized modules.
- Carry-over: every from-source module except the DWARF fixture goes through `wasm-opt`.
  A future locally-built app should adopt the same `wasm_opt_inplace` helper.
  A future *fetched* app must not.

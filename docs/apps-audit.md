# App feature audit

The ADR-24 gate: before an app binary becomes a conversion target, run

```sh
cargo run -p dewasm-core --bin feature-audit -- examples/apps/cache/*.wasm
```

and record the verdict here. An app that needs a proposal outside the 0.1
scope (wasm 1.0 + the universal baseline: sign extension, saturating
float-to-int, multi-value, bulk memory, mutable globals) is **deferred**,
not worked around; the entry stays here so it is revisited if the feature
returns.

## Verdicts (audited 2026-07-26)

| App | Source | Wasm features beyond baseline | Verdict |
| --- | --- | --- | --- |
| cowsay 0.3.0 | pinned in `fetch.sh` | none | ✅ in scope (shipping) |
| quickjs-ng v0.15.1 | pinned in `fetch.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **deepened**³) |
| sqlite3 3.53.3 (three shapes) | pinned in `fetch.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **deepened**⁴) |
| CPython 3.14.6 | pinned in `fetch-candidates.sh` | none | ✅ in scope (candidate, e2e wiring pending) |
| CRuby 3.4 (ruby.wasm 2.9.4) | pinned in `fetch-candidates.sh` | none | ✅ in scope (candidate, e2e wiring pending) |
| pandoc | see below | **simd** | ⛔ deferred |
| ripgrep | pinned-source build (planned) | n/a — audit at build time² | pending |
| zstd/gzip CLI | pinned-source build (planned) | n/a — audit at build time² | pending |

¹ **Reference-types encoding tolerance.** LLVM-based toolchains
(clang/wasi-sdk, zig, rustc) emit `call_indirect` type/table-index
immediates as padded, overlong LEBs when the `reference-types` target
feature is enabled — the default since LLVM 19. Such binaries *validate*
only with the reference-types feature bit even though they use **no
construct** from the proposal (the audit tool verifies this and prints
"encoding only"). The converter therefore keeps the validator bit enabled
as a pure encoding relaxation while rejecting every actual
reference-types construct at conversion time. Real-world wasip1 binaries
would otherwise be uniformly rejected, which would defeat the project.

² Apps we compile ourselves get their feature surface from our own build
flags; run the audit on the built artifact before wiring its e2e case.

³ **QuickJS deepened (Phase 5a).** Beyond the one-shot `-e` eval case, two
Ruby-only e2e cases now exercise real WASI filesystem use (only Ruby has
it, ADR-14) against a preopened scratch dir, both byte-identical to
`wasmtime --dir`:

- *File I/O* (`qjs_file_io_ruby`): the `qjs:std` module writes a file,
  reads it back, and prints it; the test asserts the guest stdout golden
  **and** the host-side file content. Fixture:
  `examples/apps/fixtures/qjs_file_io.js`.
- *REPL* (`qjs_repl_ruby`): **ground-truth finding — QuickJS's built-in
  interactive REPL is not usable over a pipe.** Under `wasmtime`, piping a
  scripted session (`1+2\n\q`) into bare `qjs` or `qjs -i` drives a
  terminal line editor: it emits ANSI escape sequences (cursor moves,
  syntax-highlight colors), mis-parses `1+2\q` as one line, and — the
  decisive part — never terminates on stdin EOF (it hung until a 2-minute
  timeout). So the byte-stable, pipe-friendly equivalent that is pinned
  instead is a read-eval-print loop *fixture*
  (`examples/apps/fixtures/qjs_repl.js`) reading lines from `std.in` and
  printing `std.evalScript` results — the same stdin-read + eval path a
  REPL exercises, minus the tty-only line editor. Neither case needed a
  new WASI unit; `fd_read` on stdin and the ADR-14 filesystem stack
  already cover them.

⁴ **sqlite3 deepened (Phase 5a).** The pinned source now yields **three**
artifacts (ADR-22); the DB-*file* lifecycle and a guest→host callback are
now covered, all Ruby-only and on the existing ADR-14 syscall set (no new
WASI unit):

- *Shell DB file* (`sqlite3_shell_dbfile_ruby`): one invocation creates
  and populates `/db/test.db`, a second reopens it and SELECTs; asserts
  the second run's stdout (golden vs. `wasmtime --dir`) and a nonzero DB
  file on the host.
- *Library DB file* (`sqlite3_file_c_api_ruby`): the same file
  create/close/reopen/select through the sqlite3 C API, proving the C-API
  path hits the same fs stack (fixed-string expectation — a C-API flow
  `wasmtime` cannot drive).
- *Callback binding* (`sqlite3_callback_binding_ruby`): the new
  `sqlite3-binding.wasm` (our own `examples/apps/src/sqlite3_binding.c`)
  exports `run_query`, which calls `sqlite3_exec` with a C callback
  forwarding each row to an imported `env.host_row`; the Ruby side
  provides `host_row` via the ADR-7 import-provider mechanism and collects
  the rows (fixed-string expectation).

## Deferred: pandoc

- Source: https://haskell-wasm.github.io/pandoc-wasm/pandoc.wasm
  (gh-pages of `haskell-wasm/pandoc-wasm`, unversioned — record the
  serving commit when pinning; audited copy: commit `ed18ae6e337d`,
  sha256 `48d9ceed3ef805f6acc28e6f58c2439cdeb1f71864244fffcc155e2c045aa7fc`, 53 MB).
- Audit: **needs simd** (first offense: a v128 operation at offset
  0x24723a). Notably it does *not* need tail calls or exception handling
  — the GHC 9.12 wasm backend output is otherwise baseline-shaped — so
  SIMD support alone would unblock it.
- Revisit when/if SIMD enters scope; the binary is otherwise a pure
  wasip1 stdio converter and would make a strong demo.

## WASI p1 import surfaces (for the Phase 5 wiring)

The audit also prints each binary's imported WASI functions; the widest
candidates are:

- **CPython**: 42 functions — the full p1 surface including `fd_pread`/
  `fd_pwrite`/`fd_tell`/`fd_advise`/`fd_datasync`, `path_link`/`path_rename`/
  `path_symlink`, `sched_yield`, and the four `sock_*` functions.
- **CRuby**: 37 functions — CPython's list minus the `sock_*` family,
  `sched_yield`, and `fd_filestat_set_times`, plus `fd_renumber`.

Importing is not calling: the out-of-scope `sock_*` imports still need
link-time stubs that return `ENOSYS`, but a runtime implementation is not
required for scripts that never open sockets. Both runtimes also read
their stdlib trees from a preopened directory at startup — the e2e cases
must extract and preopen those trees (`fetch-candidates.sh` notes the
paths).

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
| CPython 3.14.6 | pinned in `fetch.sh` | none | ✅ in scope (shipping, **executes on Ruby**⁵) |
| CRuby 3.4 (ruby.wasm 2.9.4) | pinned in `fetch.sh` | none | ✅ in scope (shipping, **executes on Ruby**⁵) |
| pandoc | see below | **simd** | ⛔ deferred |
| ripgrep 14.1.1 | pinned-source cargo build in `fetch.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, Ruby + Python fs⁶) |
| minigzip (zlib 1.3.1) | pinned-source zig build in `fetch.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **all three backends**⁷) |

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
e2e cases now exercise real WASI filesystem use against a preopened scratch
dir, both byte-identical to `wasmtime --dir`. Originally Ruby-only (ADR-14);
Python now mirrors both (`qjs_file_io_python`, `qjs_repl_python`, ADR-28's
third milestone adopting ADR-14's fs model) against the same fixtures and
goldens, gated behind `DEWASM_APPS_ALL` in the Python crate:

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
now covered on the existing ADR-14 syscall set (no new WASI unit). The
shell DB-file case is also mirrored under Python (`sqlite3_shell_dbfile_python`,
same fixture/golden, `DEWASM_APPS_ALL`-gated); the C-API/callback cases stay
Ruby-only (they exercise Ruby's provider/guest-memory idioms, not new WASI fs):

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

⁵ **CPython / CRuby executed on Ruby (Phase 5b).** Both language-runtime
binaries are converted *and run* on the Ruby backend, each reading its
stdlib from a preopened directory (`fetch.sh` extracts the trees:
`cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`).
The feature audit reports both as baseline-only (in scope). No new WASI unit
was needed: the wide import lists below include functions the Ruby runtime
does not implement (CPython imports `poll_oneoff`/`path_link`/`path_symlink`/
`path_readlink`/`sock_*`; CRuby imports `fd_renumber`/`poll_oneoff`/
`path_readlink`), but none is *called* on the interpreter boot + one-liner
path, so the ADR-14 syscall set already suffices (measured by running to
success). Measured on the dev machine (Apple Silicon):

| App | wasm size | convert time | Ruby source size | run time | output |
| --- | --- | --- | --- | --- | --- |
| CPython 3.14.6 | 30 MB | ~1.4 s | ~199 MB | ~12 s | `hello from cpython 42` |
| CRuby 3.4 | 35 MB | ~2.8 s | ~335 MB | ~60 s | `hello from cruby 42` |

Both are comfortably under the ADR-24 ~5-minute practicality bar, so both
genuinely execute rather than being conversion-only smokes. Because a
~335 MB temp source file and ~1 GB RSS on *every* `cargo test` is too costly
for the default gate, the execution cases (`cpython_hello_ruby`,
`cruby_hello_ruby` in the Ruby crate's e2e) are gated behind
`DEWASM_APPS_ALL` — the same deliberate perf opt-out the heavy `apps` cases
use (ADR-15's documented scope: a perf gate, not a missing-environment skip),
skip-with-a-note when unset. CRuby is the "Ruby on Ruby" north-star demo.

⁶ **ripgrep (Phase 5b).** ripgrep 14.1.1 built from the pinned source
release with `cargo build --release --target wasm32-wasip1` (default
features — which already exclude pcre2; no tweaks needed). Audit:
reference-types *encoding only*¹, in scope. A recursive directory search over
the committed fixture tree (`examples/apps/fixtures/rg/`) staged into a scratch
dir and preopened at `/work`, asserting the guest stdout is byte-identical to
the `wasmtime --dir` golden. `--sort path` forces ripgrep's otherwise-parallel
walk into a single deterministic order (without it the file order varies
run-to-run). ripgrep imports `poll_oneoff`/`path_readlink` but does not call
them on this path, so no new WASI unit was needed. Ruby (`rg_search_ruby`):
convert ~1.4 s, run ~1.7 s. Python now mirrors it (`rg_search_python`, same
fixture/golden, `DEWASM_APPS_ALL`-gated): ~10 s convert+run for the 22 MB wasm.

⁷ **minigzip / zlib (Phase 5b compression CLI).** zlib 1.3.1's `minigzip`
built from the pinned source release with `zig cc -target wasm32-wasi` (the
zlib translation units + `test/minigzip.c`; `-DZ_HAVE_UNISTD_H` so the
shipped `zconf.h` declares `lseek`). Integer-only and tiny, with **binary**
stdin/stdout — the byte-exact-stdio stress that runs under **all three**
backends (`run_gzip_cases`, wired via `gzip_e2e!` in the Ruby, Bash, and
Python crates). Two cases: *compress*
(stdin text → gz stdout byte-identical to the `wasmtime` golden
`examples/apps/golden/minigzip_compress.gz`) and *round trip* (compress then
`-d` decompress → original, self-checking). zlib's gz stream is deterministic
here — mtime 0, OS byte 3 — so wasmtime and both backends agree byte-for-byte.
Not marked heavy: no softfloat, so Bash runs it too (compress ~0.9 s + round
trip ~0.3 s under Bash). The binary stdin/golden cannot travel through the
`&str`/`include_str!` `APP_CASES` path, so these live in a dedicated
`run_gzip_cases` (bytes-capable `run_bytes`/`run_command_bytes` helpers) that
each backend calls.

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

Importing is not calling: the out-of-scope `sock_*` imports still resolve to
the ENOSYS stub, but a runtime implementation is not required for scripts
that never open sockets — confirmed by running both to success (footnote ⁵).
Both runtimes read their stdlib trees from a preopened directory at startup;
`fetch.sh` now extracts those trees (`cache/cpython-lib/lib/python3.14`,
`cache/ruby-lib/usr/local/lib/ruby`) and the e2e cases preopen them at guest
`/lib` and `/usr` respectively.

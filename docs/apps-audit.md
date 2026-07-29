# App feature audit

The ADR-24 gate: before an app binary becomes a conversion target, run

```sh
cargo run -p dewasm-core --bin feature-audit -- examples/apps/cache/*.wasm
```

and record the verdict here. An app that needs a proposal outside the 0.1 scope (wasm 1.0 + the universal baseline: sign extension, saturating float-to-int, multi-value, bulk memory, mutable globals) is **deferred**, not worked around; the entry stays here so it is revisited if the feature returns.

## Verdicts (audited 2026-07-26)

| App | Source | Wasm features beyond baseline | Verdict |
| --- | --- | --- | --- |
| cowsay 0.3.0 | pinned in `fetch.sh` | none | ✅ in scope (shipping) |
| quickjs-ng v0.15.1 | pinned in `fetch.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **deepened**³) |
| sqlite3 3.53.3 (three shapes) | pinned in `fetch.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **deepened**⁴) |
| CPython 3.14.6 | pinned in `fetch.sh` | none | ✅ in scope (shipping, **executes on Ruby/Python/Go**⁵) |
| CRuby 3.4 (ruby.wasm 2.9.4) | pinned in `fetch.sh` | none | ✅ in scope (shipping, **executes on Ruby/Python**⁵) |
| pandoc | see below | **simd** | ⛔ deferred |
| zeroperl (Perl 5.42) | see below | none (host-shim blocked) | ⛔ deferred |
| LightningCSS | see below | unaudited (unverified fork build) | ⛔ deferred |
| ripgrep 14.1.1 | pinned-source cargo build in `fetch.sh` | none (baseline after ADR-39 wasm-opt)¹¹ | ✅ in scope (shipping, Ruby + Python + Go + Java fs⁶) |
| minigzip (zlib 1.3.1) | pinned-source zig build in `fetch.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **all five backends**⁷) |
| libpcap 1.10.6 (BPF filter compiler) | pinned-source zig reactor build in `fetch.sh` | none (baseline after ADR-39 wasm-opt)¹¹ | ✅ in scope (shipping, C-API on Ruby + Python + Go⁸) |
| tree-sitter 0.26.11 + tree-sitter-json 0.24.8 | pinned-source zig reactor build in `fetch.sh` | none (baseline after ADR-39 wasm-opt)¹¹ | ✅ in scope (shipping, C-API on Ruby + Python + Go¹⁰) |

¹ **Reference-types encoding tolerance.** LLVM-based toolchains (clang/wasi-sdk, zig, rustc) emit `call_indirect` type/table-index immediates as padded, overlong LEBs when the `reference-types` target feature is enabled — the default since LLVM 19. Such binaries *validate* only with the reference-types feature bit even though they use **no construct** from the proposal (the audit tool verifies this and prints "encoding only"). The converter therefore keeps the validator bit enabled as a pure encoding relaxation while rejecting every actual reference-types construct at conversion time. Real-world wasip1 binaries would otherwise be uniformly rejected, which would defeat the project.

² Apps we compile ourselves get their feature surface from our own build flags; run the audit on the built artifact before wiring its e2e case.

³ **QuickJS deepened (Phase 5a).** Beyond the one-shot `-e` eval case, two e2e cases now exercise real WASI filesystem use against a preopened scratch dir, both byte-identical to `wasmtime --dir`. Originally Ruby-only (ADR-14); Python (`qjs_file_io_python`, `qjs_repl_python`, ADR-28), Go (`qjs_file_io_go`, `qjs_repl_go`, ADR-29's third milestone), and Java (`qjs_file_io_java`, `qjs_repl_java`, ADR-30's third milestone, all adopting ADR-14's fs model) now mirror both against the same fixtures and goldens, gated behind the `slow_test` cargo feature in each crate (`#[ignore]`d otherwise; run with `--features slow_test` or `-- --include-ignored`):

- *File I/O* (`qjs_file_io_ruby`): the `qjs:std` module writes a file, reads it back, and prints it; the test asserts the guest stdout golden **and** the host-side file content. Fixture: `examples/apps/fixtures/qjs_file_io.js`.
- *REPL* (`qjs_repl_ruby`): **ground-truth finding — QuickJS's built-in interactive REPL is not byte-stable over a pipe.** Under `wasmtime`, piping a scripted session (`1+2\n\q`) into bare `qjs` or `qjs -i` drives a terminal line editor: it emits ANSI escape sequences (cursor moves, syntax-highlight colors), mis-parses `1+2\q` as one line, and never terminates on stdin EOF over a pipe (it hangs until a timeout). A scripted read-eval-print *fixture* (`examples/apps/fixtures/qjs_repl.js`) reading lines from `std.in` and printing `std.evalScript` results exercises the same stdin-read + eval path minus the tty-only line editor, and needs nothing beyond `fd_read` and the ADR-14 filesystem stack.

The *interactive* REPL (bare `qjs`, its actual line editor) is a separate matter: after each prompt it blocks in `poll_oneoff` on an fd_read subscription over stdin, so with `poll_oneoff` resolving to ENOSYS the event loop collapsed and the program exited immediately — the fixture above was a workaround, not the end state. `poll_oneoff` is now implemented (ADR-14 revision; Ruby/Python/Go/Java, Bash deferred), and over a pipe the converted REPL now blocks on stdin, reads input, and evaluates it (the line editor still echoes ANSI escapes over a pipe, as it does under wasmtime). The interactive REPL is now verified **byte-identical to wasmtime under a real pty** on all four poll_oneoff backends (Ruby, Python, Go, Java): the `qjs_repl_pty` case (`qjs_repl_pty_e2e!`, gated on the `slow_test` feature) converts bare qjs to a standalone program, drives the scripted session `1+2⏎[3,1,2].sort()⏎Math.max(4,9)⏎\q⏎` under an 80x24 pty (`crates/dewasm-test-helper/src/pty.rs`, `portable-pty`), and compares the transcript — ANSI escapes and all — to `examples/apps/golden/qjs_repl_interactive.transcript` (2089 bytes, captured from wasmtime and re-checked by the `wasmtime_test`-gated `qjs_repl_interactive_golden` freshness test). Getting Ruby and Python green required one fix: their `fd_read` used a buffered read that blocks until the full requested length or EOF, which deadlocks a line-buffered tty that never sends EOF — stdin now uses a short read (`IO#readpartial` / `os.read`), the WASI semantics wasmtime already follows.

⁴ **sqlite3 deepened (Phase 5a).** The pinned source now yields **three** artifacts (ADR-22); the DB-*file* lifecycle and a guest→host callback are now covered on the existing ADR-14 syscall set (no new WASI unit). The shell DB-file case is also mirrored under Python (`sqlite3_shell_dbfile_python`), Go (`sqlite3_shell_dbfile_go`), and Java (`sqlite3_shell_dbfile_java`), same fixture/golden, `slow_test`-feature-gated; the C-API/callback cases stay Ruby-only (they exercise Ruby's provider/guest- memory idioms, not new WASI fs):

- *Shell DB file* (`sqlite3_shell_dbfile_ruby`): one invocation creates and populates `/db/test.db`, a second reopens it and SELECTs; asserts the second run's stdout (golden vs. `wasmtime --dir`) and a nonzero DB file on the host.
- *Library DB file* (`sqlite3_file_c_api_ruby`): the same file create/close/reopen/select through the sqlite3 C API, proving the C-API path hits the same fs stack (fixed-string expectation — a C-API flow `wasmtime` cannot drive).
- *Callback binding* (`sqlite3_callback_binding_ruby`): the new `sqlite3-binding.wasm` (our own `examples/apps/src/sqlite3_binding.c`) exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to an imported `env.host_row`; the Ruby side provides `host_row` via the ADR-7 import-provider mechanism and collects the rows (fixed-string expectation).

⁵ **CPython / CRuby executed across backends (Phase 5b; multi-backend as of the ADR-27 revision).** Both language-runtime binaries are converted *and run*, each reading its stdlib from a preopened directory (`fetch.sh` extracts the trees: `cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`). They are now shared per-case consts (`CPYTHON_HELLO`, `CRUBY_HELLO`, driven by `cpython_hello_e2e!`/`cruby_hello_e2e!`) — the stdlib trees mount straight from the app cache via the case's `cache_preopens` field (never copied), and each is ground-truthed against `wasmtime --dir`. The feature audit reports both as baseline-only (in scope). No new WASI unit was needed: the wide import lists below include functions no backend implements (CPython imports `poll_oneoff`/`path_link`/`path_symlink`/`path_readlink`/ `sock_*`; CRuby imports `fd_renumber`/`poll_oneoff`/`path_readlink`), but none is *called* on the interpreter boot + one-liner path, so the ADR-14 syscall set suffices (measured by running to success).

Measured on the dev machine (Apple Silicon), convert + (compile, compiled backends) + run:

| App | wasm | Ruby | Python | Go | Java |
| --- | --- | --- | --- | --- | --- |
| CPython 3.14.6 | 30 MB | ~1.4 s + ~12 s ✅ | ~6 s + ~31 s ✅ | ~1.4 s + `go build` ~84 s + ~1 s ✅ | ⛔ excluded |
| CRuby 3.4 | 35 MB | ~2.8 s + ~60 s ✅ | ~3 s + ~107 s ✅ | ⛔ excluded | ⛔ excluded |

Exclusions are measured, not guessed, and encoded (ADR-27 revision) as a backend simply not invoking that case's macro, the reason kept as a comment at the callsite:

- **Java (both):** a single generated interpreter method overflows the JVM's 64 KB per-method bytecode limit (`javac`: *code too large* on CPython) and, on CRuby, the element-segment `Elem` class also overflows the 64 K constant-pool limit (*too many constants*). Hard limits the ADR-30 class-splitter partitions classes — but not oversized individual methods or a single huge element segment — against.
- **Go (CRuby only):** the ~35 MB wasm's ~242 MB Go source exceeds the ADR-24 ~5-minute `go build` bar (measured >6 min). CPython, the smaller binary, clears it (~84 s).

Where included, each is comfortably under the ADR-24 ~5-minute bar, so it genuinely executes rather than being a conversion-only smoke. Because the heavy source and RSS on *every* `cargo test` is too costly for the default gate, both cases (like all filesystem-app cases) are gated behind the `slow_test` cargo feature — the same deliberate perf opt-out the slow `apps` cases use (ADR-15's documented scope: a perf gate, not a missing-environment skip). CRuby on Ruby is the "Ruby on Ruby" north-star demo.

⁶ **ripgrep (Phase 5b).** ripgrep 14.1.1 built from the pinned source release with `cargo build --release --target wasm32-wasip1` (default features — which already exclude pcre2; no tweaks needed). Audit: baseline only after the ADR-39 `wasm-opt` pass¹¹, in scope. A recursive directory search over the committed fixture tree (`examples/apps/fixtures/rg/`) staged into a scratch dir and preopened at `/work`, asserting the guest stdout is byte-identical to the `wasmtime --dir` golden. `--sort path` forces ripgrep's otherwise-parallel walk into a single deterministic order (without it the file order varies run-to-run). ripgrep imports `poll_oneoff`/`path_readlink` but does not call them on this path, so no new WASI unit was needed. Ruby (`rg_search_ruby`): convert ~1.4 s, run ~1.7 s. Python (`rg_search_python`), Go (`rg_search_go`, ADR-29), and Java (`rg_search_java`, ADR-30) now mirror it against the same fixture/golden, `slow_test`-feature-gated: Python ~10 s convert+run for the 22 MB wasm; Go convert+`go build`+run cold likewise dominated by the compile. Java is the class-split stress case: rg's ~7300 functions and ~4900-entry function table overflow a single class's 65535-entry constant pool, so its functions are partitioned across five nested `P{k}` classes and the table is built in a nested `Elem` class (ADR-30), each with its own pool; convert ~2 s, `javac` ~10 s, run warm.

⁷ **minigzip / zlib (Phase 5b compression CLI).** zlib 1.3.1's `minigzip` built from the pinned source release with `zig cc -target wasm32-wasi` (the zlib translation units + `test/minigzip.c`; `-DZ_HAVE_UNISTD_H` so the shipped `zconf.h` declares `lseek`). Integer-only and tiny, with **binary** stdin/stdout — the byte-exact-stdio stress that runs under **all five** backends (`run_gzip_cases`, wired via `gzip_e2e!` in the Ruby, Bash, Python, Go, and Java crates; Go and Java — the compiled backends — prove the byte-stdio path is exact through compiled output too, ADR-29/ADR-30). Two cases: *compress* (stdin text → gz stdout byte-identical to the `wasmtime` golden `examples/apps/golden/minigzip_compress.gz`) and *round trip* (compress then `-d` decompress → original, self-checking). zlib's gz stream is deterministic here — mtime 0, OS byte 3 — so wasmtime and every backend agree byte-for-byte. Not marked slow: no softfloat, so Bash runs it too (compress ~0.9 s + round trip ~0.3 s under Bash). The binary stdin/golden cannot travel through the `&str`/`include_str!` `APP_CASES` path, so these live in a dedicated `run_gzip_cases` (bytes-capable `run_bytes`/`run_command_bytes` helpers) that each backend calls.

⁸ **libpcap (Track A).** libpcap 1.10.6 built from the pinned upstream release with `zig cc -target wasm32-wasi -mexec-model=reactor` as a C-API library. Only the platform-independent BPF-filter-compilation translation units are compiled (no capture backend); the parser is regenerated with bison/flex (1.10.x no longer ships pre-generated `grammar.c`/`scanner.c`). Audit: baseline only after the ADR-39 `wasm-opt` pass¹¹ (which re-encodes the overlong `call_indirect` immediates), in scope. Our own `examples/apps/src/pcap_binding.c` exports `compile_filter`, which runs `pcap_compile_nopcap` and serializes the resulting BPF program (`[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]`) into guest memory; the C-API case (`pcap_compile`, `pcap_compile_e2e!`) drives `compile_filter("tcp port 80", DLT_EN10MB, 65535)` on Ruby, Python, and Go and pins the canonical tcp-port-80 program (deterministic — BPF holds offsets/constants only). Like the other reactor-library C-API cases it is `slow_test`-gated (a ~2 MB artifact reconverted per run). Bash does not participate: no host-language C API to plumb a pointer-returning binding through (ADR-12). *Shim caveat:* wasip1 has no `./configure` host, no `socket()`, and no baseline `setjmp`/`longjmp`, so a first-party `examples/apps/src/pcap_config.h`⁹ stands in for the generated `config.h` — see its header comment.

⁹ **The `pcap_config.h` shim** collapses three wasip1 gaps: the `./configure` feature macros the filter compiler reads; placeholders (`socket()`, `SIOCGIF*`) that let the never-reached, wasm-ld-GC'd `pcap_lookupnet` compile; and a baseline-wasm `setjmp`→0 / `longjmp`→trap stand-in (libpcap reports filter *syntax errors* via `longjmp`, which wasip1's `<setjmp.h>` refuses to compile without the out-of-scope wasm exception-handling proposal). A valid filter — the only kind this demo compiles — never takes the error path, so the stand-in is transparent; an invalid filter would trap rather than return an error. Name-based filters (`host example.com`) are likewise out of scope: `pcap_binding.c` stubs the missing `getaddrinfo`/`getnetbyname`/`getprotobyname` to report "not found".

¹⁰ **tree-sitter (Track A).** The tree-sitter incremental-parsing runtime 0.26.11 (single-TU amalgamation `lib/src/lib.c`) plus the pre-generated tree-sitter-json 0.24.8 grammar (`src/parser.c`), built from the pinned upstream releases with `zig cc -mexec-model=reactor` as a C-API library. Audit: baseline only after the ADR-39 `wasm-opt` pass¹¹, in scope — unlike libpcap, the runtime needs no shim (no `setjmp`, no host lookups). Our own `examples/apps/src/treesitter_binding.c` exports `parse_source`, which parses a source string and returns the parse tree's S-expression (`ts_node_string`, a malloc'd C string) into guest memory. The C-API case (`treesitter_parse`, `treesitter_parse_e2e!`) parses the fixed snippet `{"key": [1, true, null]}` on Ruby, Python, and Go and pins the S-expression `(document (object (pair key: (string (string_content)) value: (array (number) (true) (null)))))` (deterministic — tree-sitter's node naming is fixed by the pinned grammar). `slow_test`-gated like the other reactor-library C-API cases; Bash does not participate (ADR-12).

¹¹ **ADR-39 `wasm-opt` preprocessing.** The three modules `fetch.sh` builds locally (libpcap, tree-sitter, ripgrep) are run through `wasm-opt -O2` (baseline features only, no ctor-eval) before caching — see [ADR-39](adr/39-wasm-opt-preprocessing.md). Besides shrinking them, `wasm-opt` re-encodes the overlong `call_indirect` immediates the LLVM toolchain emits, so these modules audit as *pure* baseline rather than baseline + the reference-types encoding bit¹ the downloaded/unoptimized artifacts carry.

## Deferred: pandoc

- Source: https://haskell-wasm.github.io/pandoc-wasm/pandoc.wasm (gh-pages of `haskell-wasm/pandoc-wasm`, unversioned — record the serving commit when pinning; audited copy: commit `ed18ae6e337d`, sha256 `48d9ceed3ef805f6acc28e6f58c2439cdeb1f71864244fffcc155e2c045aa7fc`, 53 MB).
- Audit: **needs simd** (first offense: a v128 operation at offset 0x24723a). Notably it does *not* need tail calls or exception handling — the GHC 9.12 wasm backend output is otherwise baseline-shaped — so SIMD support alone would unblock it.
- Revisit when/if SIMD enters scope; the binary is otherwise a pure wasip1 stdio converter and would make a strong demo.

## Deferred: zeroperl

- Source: [github.com/6over3/zeroperl](https://github.com/6over3/zeroperl) — a WASI reactor build of Perl 5.42. A prebuilt artifact is redistributed in [github.com/lbe/go-exiftool-wasm](https://github.com/lbe/go-exiftool-wasm) as `internal/zeroperl/zeroperl.wasm` (pin the serving commit + sha256 when it is promoted).
- Audit: **not blocked on a wasm feature** — the blocker is host shims. The build relies on binaryen **asyncify** plus a custom **setjmp/longjmp** shim and an imported `env.call_host_function`, none of which the runtime provides. This is an ABI/host-environment gap, not a proposal outside the 0.1 scope.
- Revisit when a setjmp/asyncify story exists (a general asyncify unwinding shim plus the `call_host_function` host glue); Perl 5 would be a marquee scripting-language demo alongside CPython and CRuby.

## Deferred: LightningCSS

- Source: [github.com/pgaskin/go-lightningcss](https://github.com/pgaskin/go-lightningcss) — a Rust **reactor** build of LightningCSS (the CSS parser/transformer), produced via a **pgaskin/wasm2go fork** of the build tooling; the published artifact is therefore unverified against an upstream release.
- Audit: **not yet run** — deferred pending audit. The fork-built artifact is not trustworthy enough to promote as-is.
- Revisit by pinning the build (a reproducible from-source recipe, not the fork's prebuilt wasm) and running the feature-audit on the resulting binary before promoting it in scope.

## WASI p1 import surfaces (for the Phase 5 wiring)

The audit also prints each binary's imported WASI functions; the widest candidates are:

- **CPython**: 42 functions — the full p1 surface including `fd_pread`/ `fd_pwrite`/`fd_tell`/`fd_advise`/`fd_datasync`, `path_link`/`path_rename`/ `path_symlink`, `sched_yield`, and the four `sock_*` functions.
- **CRuby**: 37 functions — CPython's list minus the `sock_*` family, `sched_yield`, and `fd_filestat_set_times`, plus `fd_renumber`.

Importing is not calling: the out-of-scope `sock_*` imports still resolve to the ENOSYS stub, but a runtime implementation is not required for scripts that never open sockets — confirmed by running both to success (footnote ⁵). Both runtimes read their stdlib trees from a preopened directory at startup; `fetch.sh` now extracts those trees (`cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`) and the e2e cases preopen them at guest `/lib` and `/usr` respectively.

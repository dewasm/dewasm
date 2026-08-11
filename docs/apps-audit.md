# App feature audit

The scope test: before an app binary becomes a conversion target, run

```sh
cargo run -p dewasm-core --bin feature-audit -- examples/apps/cache/*.wasm
```

and record the verdict here.
An app that needs a proposal outside the 0.1 scope (wasm 1.0 + the universal baseline: sign extension, saturating float-to-int, multi-value, bulk memory, mutable globals) is **deferred**, not worked around; the entry stays here so it is revisited if the feature returns.

## Verdicts

| App | Source | Wasm features beyond baseline | Verdict |
| --- | --- | --- | --- |
| cowsay 0.3.0 | pinned in `setup.sh` | none | ✅ in scope (shipping) |
| quickjs-ng v0.15.1 | pinned in `setup.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **deepened**³) |
| sqlite3 3.53.3 (three shapes) | pinned in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, **deepened**⁴) |
| CPython 3.14.6 | pinned in `setup.sh` | none | ✅ in scope (shipping, **executes on every backend**⁵) |
| CRuby 3.4 (ruby.wasm 2.9.4) | pinned in `setup.sh` | none | ✅ in scope (shipping, **executes on every backend**⁵) |
| CRuby 3.4 wasi-vfs-packed | derived in-cache by `setup.sh`¹³ | none (audited 2026-08-04) | ✅ in scope (shipping, **executes on every backend**¹³) |
| pandoc | see below | **simd** | ⛔ deferred |
| zeroperl (Perl 5.42) | [6over3/zeroperl](https://github.com/6over3/zeroperl) via the `@6over3/zeroperl-ts` npm pin in `scripts/zeroperl.sh` | none | ✅ in scope (shipping, **executes on every backend**¹²) |
| LightningCSS | see below | unaudited (unverified fork build) | ⛔ deferred |
| ripgrep 14.1.1 | pinned-source cargo build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, fs on every backend⁶) |
| minigzip (zlib 1.3.1) | pinned-source zig build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, **every backend**⁷) |
| libpcap 1.10.6 (BPF filter compiler) | pinned-source zig reactor build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, C-API on every backend⁸) |
| tree-sitter 0.26.11 + tree-sitter-json 0.24.8 | pinned-source zig reactor build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, C-API on every backend¹⁰) |

¹ **Reference-types encoding tolerance.**
LLVM-based toolchains (clang/wasi-sdk, zig, rustc) emit `call_indirect` type/table-index immediates as padded, overlong LEBs when the `reference-types` target feature is enabled, the default since LLVM 19.
Such binaries *validate* only with the reference-types feature bit even though they use **no construct** from the proposal (the audit tool verifies this and prints "encoding only").
The converter therefore keeps the validator bit enabled as a pure encoding relaxation while rejecting every actual reference-types construct at conversion time.
Real-world wasip1 binaries would otherwise be uniformly rejected, which would defeat the project.

² Apps we compile ourselves get their feature surface from our own build flags; run the audit on the built artifact before wiring its e2e case.

³ **QuickJS deepened.**
Beyond the one-shot `-e` eval case, e2e cases exercise real WASI filesystem use against a preopened scratch dir and the interactive REPL under a real pty, both byte-identical to `wasmtime`.
Every backend mirrors the file I/O case against the same fixtures and snapshots on the shared WASI filesystem model, conditional behind the `slow_test` cargo feature in each crate (`#[ignore]`d otherwise; run with `--features slow_test` or `-- --include-ignored`):

- *File I/O* (`qjs_file_io`): the `qjs:std` module writes a file, reads it back, and prints it; the test asserts the guest stdout snapshot **and** the host-side file content.
  Fixture: `examples/apps/fixtures/qjs_file_io.js`.

**REPL.**
QuickJS's built-in interactive REPL is not byte-stable over a pipe: under `wasmtime`, piping a scripted session (`1+2\n\q`) into bare `qjs` or `qjs -i` drives a terminal line editor that emits ANSI escape sequences (cursor moves, syntax-highlight colors), mis-parses `1+2\q` as one line, and never terminates on stdin EOF over a pipe (it hangs until a timeout).
Driving it therefore requires a real pty, not a scripted stdin pipe, and it requires `poll_oneoff`, since after each prompt the REPL blocks on an fd_read subscription over stdin.
Every backend implements `poll_oneoff` and runs the case, so the interactive REPL is verified **byte-identical to wasmtime under a real pty** on all six: the `qjs_repl_pty` case (`qjs_repl_pty_e2e!`) converts bare qjs to a standalone program, drives the scripted session `1+2⏎[3,1,2].sort()⏎Math.max(4,9)⏎\q⏎` under an 80x24 pty (`crates/dewasm-test-helper/src/pty.rs`, `portable-pty`), and compares the transcript, ANSI escapes and all, to `examples/apps/snapshots/qjs_repl_interactive.transcript` (captured from wasmtime and re-checked by the `wasmtime_test`-conditional `qjs_repl_interactive_snapshot` freshness test).
Making Ruby and Python match required one fix: their `fd_read` used a buffered read that blocks until the full requested length or EOF, which deadlocks a line-buffered tty that never sends EOF, so stdin now uses a short read (`IO#readpartial` / `os.read`), the WASI semantics wasmtime already follows.

⁴ **sqlite3 deepened.**
The pinned source yields **three** artifacts; the DB-*file* lifecycle and a guest→host callback are covered on the existing WASI filesystem syscall set (no new WASI unit).
Every backend runs all three cases below, same fixtures and snapshots, `slow_test`-feature-conditional; the C-API and callback ones exercise each backend's provider and guest-memory idioms rather than new WASI fs:

- *Shell DB file* (`sqlite3_shell_dbfile`): one invocation creates and populates `/db/test.db`, a second reopens it and SELECTs; asserts the second run's stdout (snapshot vs. `wasmtime --dir`) and a nonzero DB file on the host.
- *Library DB file* (`sqlite3_file_c_api`): the same file create/close/reopen/select through the sqlite3 C API, proving the C-API path hits the same fs stack (fixed-string expectation, since `wasmtime` cannot drive a C-API flow).
- *Callback binding* (`sqlite3_callback_binding`): `sqlite3-binding.wasm` (our own `examples/apps/src/sqlite3_binding.c`) exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to an imported `env.host_row`; the host side provides `host_row` via the import-provider mechanism and collects the rows (fixed-string expectation).

⁵ **CPython / CRuby executed across backends.**
Both language-runtime binaries are converted *and run*, each reading its stdlib from a preopened directory (`setup.sh` extracts the trees: `cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`).
They are shared per-case consts (`CPYTHON_HELLO`, `CRUBY_HELLO`, driven by `cpython_hello_e2e!`/`cruby_hello_e2e!`); the stdlib trees mount straight from the app cache via the case's `cache_preopens` field (never copied), and each is ground-truthed against `wasmtime --dir`.
The feature audit reports both as baseline-only (in scope).
No new WASI unit was needed: the wide import lists below include functions no backend implements (CPython imports `poll_oneoff`/`path_link`/`path_symlink`/`path_readlink`/ `sock_*`; CRuby imports `fd_renumber`/`poll_oneoff`/`path_readlink`), but none is *called* on the interpreter boot + one-liner path, so the implemented syscall set suffices (measured by running to success).

Every backend runs both cases; what varies is the speed category.
Where a case sits at `ultra` (both cases on Go, Java and Bash, plus CRuby on Perl) the cost is wall time, not feasibility: the output is correct, so those runs stay out of CI and happen by hand before a release.

Java needed two splitter fixes to run them at all: CPython's largest function holds a 3202-target `br_table`, one statement whose `switch` alone passes the 64 KB per-method limit (*code too large*) with no statement boundary to split at, and CRuby's 8737-entry funcref table saturates one class's 65535-entry constant pool (*too many constants*).
The splitter now cuts a `br_table` at its case ranges and spreads the table's fillers over `ElemF{c}` classes; both are general, not app-specific.

Bash needs one thing no other Bash case does: `ulimit -s` raised in the glue.
Every wasm call nests a native bash call, so CPython's boot otherwise exhausts the 8 MB process stack and dies of SIGSEGV.
The generated standalone entrypoint raises it already; a library-mode embedder has to do it itself.

⁶ **ripgrep.**
ripgrep 14.1.1 built from the pinned source release with `cargo build --release --target wasm32-wasip1` (default features, which already exclude pcre2; no tweaks needed).
Audit: baseline only after the `wasm-opt` pass¹¹, in scope.
The `rg_search` case is a recursive directory search over the committed fixture tree (`examples/apps/fixtures/rg/`) staged into a scratch dir and preopened at `/work`, asserting the guest stdout is byte-identical to the `wasmtime --dir` snapshot.
`--sort path` forces ripgrep's otherwise-parallel walk into a single deterministic order (without it the file order varies run-to-run).
ripgrep imports `poll_oneoff`/`path_readlink` but does not call them on this path, so no new WASI unit was needed.
Every backend runs it against the same fixture and snapshot, `slow_test`-feature-conditional.
Java is the class-split stress case: rg's ~7300 functions and ~4900-entry function table overflow a single class's 65535-entry constant pool, so its functions are partitioned across five nested `P{k}` classes and the table is built in a nested `Elem` class, each with its own pool.

⁷ **minigzip / zlib (compression CLI).**
zlib 1.3.1's `minigzip` built from the pinned source release with `zig cc -target wasm32-wasi` (the zlib translation units + `test/minigzip.c`; `-DZ_HAVE_UNISTD_H` so the shipped `zconf.h` declares `lseek`).
Integer-only and tiny, with **binary** stdin/stdout: the byte-exact-stdio stress that runs under **every** backend (`run_gzip_cases`, wired via `gzip_e2e!` in each crate; the compiled backends, Go and Java, prove the byte-stdio path is exact through compiled output too).
Two cases: *compress* (stdin text → gz stdout byte-identical to the `wasmtime` snapshot `examples/apps/snapshots/minigzip_compress.gz`) and *round trip* (compress then `-d` decompress → original, self-checking).
zlib's gz stream is deterministic here (mtime 0, OS byte 3), so wasmtime and every backend agree byte-for-byte.
Not marked slow: no softfloat, so Bash runs it too.
The binary stdin/snapshot cannot travel through the `&str`/`include_str!` `APP_CASES` path, so these live in a dedicated `run_gzip_cases` (bytes-capable `run_bytes`/`run_command_bytes` helpers) that each backend calls.

⁸ **libpcap (Track A).**
libpcap 1.10.6 built from the pinned upstream release with `zig cc -target wasm32-wasi -mexec-model=reactor` as a C-API library.
Only the platform-independent BPF-filter-compilation translation units are compiled (no capture backend); the parser is regenerated with bison/flex (1.10.x no longer ships pre-generated `grammar.c`/`scanner.c`).
Audit: baseline only after the `wasm-opt` pass¹¹ (which re-encodes the overlong `call_indirect` immediates), in scope.
Our own `examples/apps/src/pcap_binding.c` exports `compile_filter`, which runs `pcap_compile_nopcap` and serializes the resulting BPF program (`[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]`) into guest memory; the C-API case (`pcap_compile`, `pcap_compile_e2e!`) drives `compile_filter("tcp port 80", DLT_EN10MB, 65535)` on every backend and pins the canonical tcp-port-80 program (deterministic: BPF holds offsets and constants only).
Like the other reactor-library C-API cases it is `slow_test`-conditional.
Bash drives it like the rest: a guest pointer is a decimal in the `R0` result global and guest memory is the module's byte array, so the walk over the serialized program is plain shell arithmetic.
*Shim caveat:* wasip1 has no `./configure` host, no `socket()`, and no baseline `setjmp`/`longjmp`, so a first-party `examples/apps/src/pcap_config.h`⁹ stands in for the generated `config.h`; see its header comment.

⁹ **The `pcap_config.h` shim** collapses three wasip1 gaps: the `./configure` feature macros the filter compiler reads; placeholders (`socket()`, `SIOCGIF*`) that let the never-reached, wasm-ld-GC'd `pcap_lookupnet` compile; and a baseline-wasm `setjmp`→0 / `longjmp`→trap stand-in (libpcap reports filter *syntax errors* via `longjmp`, which wasip1's `<setjmp.h>` refuses to compile without the out-of-scope wasm exception-handling proposal).
A valid filter, the only kind this demo compiles, never takes the error path, so the stand-in is transparent; an invalid filter would trap rather than return an error.
Name-based filters (`host example.com`) are likewise out of scope: `pcap_binding.c` stubs the missing `getaddrinfo`/`getnetbyname`/`getprotobyname` to report "not found".

¹⁰ **tree-sitter (Track A).**
The tree-sitter incremental-parsing runtime 0.26.11 (single-TU amalgamation `lib/src/lib.c`) plus the pre-generated tree-sitter-json 0.24.8 grammar (`src/parser.c`), built from the pinned upstream releases with `zig cc -mexec-model=reactor` as a C-API library.
Audit: baseline only after the `wasm-opt` pass¹¹, in scope; unlike libpcap, the runtime needs no shim (no `setjmp`, no host lookups).
Our own `examples/apps/src/treesitter_binding.c` exports `parse_source`, which parses a source string and returns the parse tree's S-expression (`ts_node_string`, a malloc'd C string) into guest memory.
The C-API case (`treesitter_parse`, `treesitter_parse_e2e!`) parses the fixed snippet `{"key": [1, true, null]}` on every backend and pins the S-expression `(document (object (pair key: (string (string_content)) value: (array (number) (true) (null)))))` (deterministic: tree-sitter's node naming is fixed by the pinned grammar).
`slow_test`-conditional like the other reactor-library C-API cases, Bash included (see footnote 8 for how the pointer plumbing reads in shell).

¹¹ **`wasm-opt` preprocessing.**
Every module `setup.sh` builds from source (the three sqlite3 shapes, minigzip, libpcap, tree-sitter, ripgrep, but not the DWARF fixture, which keeps its debug info) is run through `wasm-opt -O2` before caching, baseline features only and no ctor-eval.
Besides shrinking them, `wasm-opt` re-encodes the overlong `call_indirect` immediates the LLVM toolchain emits, so these modules audit as *pure* baseline rather than baseline + the reference-types encoding bit¹ the downloaded artifacts (cowsay, qjs, CPython, CRuby) still carry.

¹² **zeroperl retraction (audited 2026-08-01).**
This entry was previously *deferred* on three presumed host-environment blockers; converting and running the module proved all three were misreadings, so the verdict is retracted.
(1) **asyncify** and the **setjmp/longjmp** shim are module-internal: asyncify is a binaryen transform baked into the wasm, and the setjmp implementation is a port of ruby.wasm's `rb_wasm_setjmp` that lowers to ordinary baseline instructions.
(2) The imported **`env.call_host_function`** is only invoked when the guest registers a host callback, which the eval path never does, so a zero-returning stub as an import provider satisfies the link with no host glue.
(3) No **stdlib preopen** is needed: the Perl core is embedded in the module as an "SFS" blob served from guest memory, and the only preopen `zeroperl_init` requires is `/dev/null` (mapped guest→host `/dev/null`; without it init returns 1).

The prebuilt reactor exposes an embedding C API, so it is driven exactly like the other reactor-library C-API cases (footnotes ⁸/¹⁰): the `zeroperl_eval` case (`zeroperl_eval_e2e!`) evaluates a regex + `printf` Perl program and pins its stdout.
The pinnable distribution is the `@6over3/zeroperl-ts` npm wrapper, since the `6over3/zeroperl` source repo cuts no releases; MIT source, Apache-2.0 npm wrapper.
The `exiftool_extract` case (`exiftool_extract_e2e!`) runs a *real* Perl application on the same converted reactor: the flattened ExifTool 13.42 CLI driver (6over3/exiftool `src/exiftool`, fetched into `cache/exiftool-lib/`), preopened at `/work` alongside a committed EXIF image fixture at `/img`, extracts deterministic tags (`-S -Make -Model -DateTimeOriginal`, cross-checked against host exiftool).
It exercises the SFS + preopen path end to end (`use Image::ExifTool` resolves from the module tree embedded in the SFS blob, while the driver script and image come in through real WASI preopens) and confirms the C-API drive survives ExifTool's terminal `exit`, overridden to a `die` so it unwinds into `eval_pv` rather than tripping `proc_exit`.
It reconverts the cached `zeroperl.wasm`, so it adds no convert-suite row.

Both cases run on all six backends, at speeds from `slow` to `ultra`.
*Preopen caveat:* the `/dev/null` preopen is not a directory, which the Python, Go, Java, and Bash runtimes rejected; each now only requires a preopen path to *resolve*, as Ruby and Perl already did.
Python and Bash additionally collapse a final `.` component during path resolution, because wasi-libc rewrites a path that *is* a preopen to `.` and `os.open("/dev/null/.")` is ENOTDIR, where Ruby's `File.realpath`, Perl's `Cwd::realpath`, and Go's `filepath.Join` already collapsed it.
On Java the reactor is what pushed the function-partition threshold down to 2000: its ~2450 constant-dense functions overflow a single class's 65535-entry pool (`javac`: *too many constants*).

¹³ **CRuby wasi-vfs-packed (audited 2026-08-04).**
ruby.wasm's intended deployment shape: `setup.sh` packs the two already-pinned CRuby artifacts (`cache/ruby.wasm` plus the `cache/ruby-lib/usr` stdlib tree) into the self-contained `cache/ruby-packed.wasm` with the pinned `wasi-vfs` CLI.
The official build links `libwasi_vfs.a`, and `wasi-vfs pack` embeds the tree via wizer pre-initialization as ordinary data segments, so the audit is baseline-only with the same 37-function import list as the unpacked CRuby.
Needing **no preopens at all**, the case (`CRUBY_PACKED_HELLO`, `cruby_packed_hello_e2e!`) is a plain `AppCase`, a stdlib `require "json"` one-liner proving the embedded VFS serves the tree, ground-truthed by the `wasmtime_test` suite with zero `--dir` flags.
Every backend runs it; it is faster than the unpacked case wherever both are measured, because the stdlib loads from guest memory instead of host I/O, but the speed categories still vary by backend, and on Python the constraint is host memory rather than the clock.

## Deferred: pandoc

- Source: https://haskell-wasm.github.io/pandoc-wasm/pandoc.wasm (gh-pages of `haskell-wasm/pandoc-wasm`, unversioned, so record the serving commit when pinning; audited copy: commit `ed18ae6e337d`, sha256 `48d9ceed3ef805f6acc28e6f58c2439cdeb1f71864244fffcc155e2c045aa7fc`, 53 MB).
- Audit: **needs simd** (first offense: a v128 operation at offset 0x24723a).
  Notably it does *not* need tail calls or exception handling (the GHC 9.12 wasm backend output is otherwise baseline-shaped), so SIMD support alone would unblock it.
- Revisit when/if SIMD enters scope; the binary is otherwise a pure wasip1 stdio converter and would make a strong demo.

## Deferred: LightningCSS

- Source: [github.com/pgaskin/go-lightningcss](https://github.com/pgaskin/go-lightningcss), a Rust **reactor** build of LightningCSS (the CSS parser/transformer), produced via a **pgaskin/wasm2go fork** of the build tooling; the published artifact is therefore unverified against an upstream release.
- Audit: **not yet run**, deferred pending audit.
  The fork-built artifact is not trustworthy enough to promote as-is.
- Revisit by pinning the build (a reproducible from-source recipe, not the fork's prebuilt wasm) and running the feature-audit on the resulting binary before promoting it in scope.

## WASI p1 import surfaces

The audit also prints each binary's imported WASI functions; the widest candidates are:

- **CPython**: 42 functions, the full p1 surface including `fd_pread`/ `fd_pwrite`/`fd_tell`/`fd_advise`/`fd_datasync`, `path_link`/`path_rename`/ `path_symlink`, `sched_yield`, and the four `sock_*` functions.
- **CRuby**: 37 functions, CPython's list minus the `sock_*` family, `sched_yield`, and `fd_filestat_set_times`, plus `fd_renumber`.

Importing is not calling: the out-of-scope `sock_*` imports still resolve to the ENOSYS stub, but a runtime implementation is not required for scripts that never open sockets, confirmed by running both to success (footnote ⁵).
Both runtimes read their stdlib trees from a preopened directory at startup; `setup.sh` now extracts those trees (`cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`) and the e2e cases preopen them at guest `/lib` and `/usr` respectively.

# App feature audit

The scope test: before an app binary becomes a conversion target, run

```sh
cargo xtask feature-audit examples/apps/cache/*.wasm
```

and record the verdict here.
An app that needs a proposal outside the accepted input is **deferred**, not worked around.
Its entry stays here so it is revisited if the feature returns.
The accepted input is wasm 1.0 plus the following:

- the universal baseline:
  - sign extension;
  - saturating float-to-int;
  - multi-value;
  - bulk memory;
  - mutable globals;
- the final exception-handling proposal and the tail-call proposal, both lowered per backend.

The same verdict covers an app with no artifact to run the audit tool on at all.
That holds when every published build is broken and a from-source build fails in the toolchain.
The entry then records that evidence so the investigation is not repeated.

## Verdicts

| App | Source | Wasm features beyond baseline | Verdict |
| --- | --- | --- | --- |
| cowsay.wasm 0.2.0 (cowsay 3.03 in C) | our own release, pinned in `setup.sh` | none | ✅ in scope (shipping) |
| quickjs-ng v0.15.1 | pinned in `setup.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **deepened**³) |
| sqlite3 3.53.3 (three shapes) | pinned in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, **deepened**⁴) |
| CPython 3.14.6 | pinned in `setup.sh` | none | ✅ in scope (shipping, **executes on every backend**⁵) |
| CRuby 3.4 (ruby.wasm 2.9.4) | pinned in `setup.sh` | none | ✅ in scope (shipping, **executes on every backend**⁵) |
| CRuby 3.4 wasi-vfs-packed | derived in-cache by `setup.sh`¹³ | none (audited 2026-08-04) | ✅ in scope (shipping, **executes on every backend**¹³) |
| mruby 3.4.0 | pinned-source wasi-sdk build in `setup.sh` | **exception-handling** (accepted input¹⁴), reference-types *encoding only*¹ | ✅ in scope (shipping, **executes on every backend but Bash**¹⁴) |
| pandoc | see below | **simd** | ⛔ deferred |
| zeroperl (Perl 5.42) | [6over3/zeroperl](https://github.com/6over3/zeroperl) via the `@6over3/zeroperl-ts` npm pin in `scripts/zeroperl.sh` | none | ✅ in scope (shipping, **executes on every backend**¹²) |
| LightningCSS | see below | unaudited (unverified fork build) | ⛔ deferred |
| ripgrep 14.1.1 | pinned-source cargo build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, fs on every backend⁶) |
| minigzip (zlib 1.3.1) | pinned-source wasi-sdk build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, **every backend**⁷) |
| libpcap 1.10.6 (BPF filter compiler) | pinned-source wasi-sdk reactor build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, C-API on every backend⁸) |
| tree-sitter 0.26.11 + tree-sitter-json 0.24.8 | pinned-source wasi-sdk reactor build in `setup.sh` | none (baseline after the wasm-opt pass)¹¹ | ✅ in scope (shipping, C-API on every backend¹⁰) |
| Lua 5.4.7 | see below | no artifact to audit (SjLj build crashes wasm-ld, prebuilts broken) | ⛔ deferred |
| PHP | see below | no artifact to audit (no maintained wasm32-wasip1 build) | ⛔ deferred |
| toywasm 76.0.0 | pinned in `setup.sh` | reference-types *encoding only*¹ | ✅ in scope (shipping, **executes on every backend**¹⁵) |
| wasm3 0.9.0 | official `wasm3-wasi.wasm` release asset, pinned in `setup.sh` | **tail calls** (accepted input¹⁶) | ✅ in scope (shipping, **executes on every backend**¹⁶) |


¹ **Reference-types encoding tolerance.**
The `reference-types` target feature is enabled by default since LLVM 19.
With it, LLVM-based toolchains (clang/wasi-sdk, zig, rustc) emit padded, overlong LEBs.
These LEBs encode the type and table-index immediates of `call_indirect`.
Such binaries use **no construct** from the reference-types proposal.
Even so, they *validate* only with its feature bit.
The audit tool verifies this and prints "encoding only".
The converter therefore keeps the validator bit enabled as a pure encoding relaxation.
It still rejects every actual reference-types construct at conversion time.
Real-world wasip1 binaries would otherwise be uniformly rejected, which would defeat the project.

² Apps we compile ourselves get their feature surface from our own build flags.
Run the audit on the built artifact before adding its e2e case.

³ **QuickJS deepened.**
Two e2e cases go beyond the one-shot `-e` eval case.
Both are byte-identical to `wasmtime`.
One exercises real WASI filesystem use against a preopened scratch dir.
The other exercises the interactive REPL under a real pty.

Every backend mirrors the file I/O case against the same fixtures and snapshots.
It runs on the shared WASI filesystem model.
Each crate makes it conditional on the `slow_test` cargo feature, and `#[ignore]`s it otherwise.
Run it with `--features slow_test`.

- *File I/O* (`qjs_file_io`): the `qjs:std` module writes a file, reads it back, and prints it.
  The test asserts the guest stdout snapshot **and** the host-side file content.
  Fixture: `examples/apps/fixtures/qjs_file_io.js`.

**REPL.**
QuickJS's built-in interactive REPL is not byte-stable over a pipe.
Under `wasmtime`, a scripted session (`1+2\n\q`) piped into bare `qjs` or `qjs -i` drives a line editor.
That terminal line editor does three things:

- it emits ANSI escape sequences (cursor moves, syntax-highlight colors);
- it mis-parses `1+2\q` as one line;
- it never terminates on stdin EOF over a pipe (it hangs until a timeout).

Driving it therefore requires a real pty, not a scripted stdin pipe.
After each prompt the REPL blocks on an fd_read subscription over stdin.
So driving it also requires `poll_oneoff`.

Every backend implements `poll_oneoff` and runs the case.
So the interactive REPL is verified **byte-identical to wasmtime under a real pty** on all six.
The `qjs_repl_pty` case (`qjs_repl_pty_e2e!`) does three things:

- it converts bare qjs to a standalone program;
- it drives the scripted session `1+2⏎[3,1,2].sort()⏎Math.max(4,9)⏎\q⏎` under an 80x24 pty;
- it compares the transcript, ANSI escapes and all, to a snapshot.

The pty comes from `crates/dewasm-test-helper/src/pty.rs` (`portable-pty`).
The snapshot is `examples/apps/snapshots/qjs_repl_interactive.transcript`.
It was captured from wasmtime.
The `wasmtime_test`-conditional `qjs_repl_interactive_snapshot` freshness test re-checks it.

Making Ruby and Python match required one fix to their `fd_read`.
It used a buffered read that blocks until the full requested length or EOF.
That deadlocks a line-buffered tty that never sends EOF.
So stdin now uses a short read (`IO#readpartial` / `os.read`).
That is the WASI semantics wasmtime already follows.

⁴ **sqlite3 deepened.**
The pinned source yields **three** artifacts.
The existing WASI filesystem syscall set covers the DB-*file* lifecycle and a guest→host callback.
They need no new WASI unit.
Every backend runs all three cases below on the same fixtures and snapshots.
Each case is `slow_test`-feature-conditional.
The C-API and callback cases exercise each backend's provider and guest-memory idioms.
They do not exercise new WASI fs.

- *Shell DB file* (`sqlite3_shell_dbfile`): one invocation creates and populates `/db/test.db`.
  A second reopens it and SELECTs.
  The case asserts the second run's stdout against a `wasmtime --dir` snapshot.
  It also asserts a nonzero DB file on the host.
- *Library DB file* (`sqlite3_file_c_api`): the same file create/close/reopen/select.
  It runs through the sqlite3 C API.
  It proves the C-API path hits the same fs stack.
  Its expectation is a fixed string, since `wasmtime` cannot drive a C-API flow.
- *Callback binding* (`sqlite3_callback_binding`): `sqlite3-binding.wasm` exports `run_query`.
  Its source is our own `examples/apps/src/sqlite3_binding.c`.
  `run_query` calls `sqlite3_exec` with a C callback forwarding each row to an imported `env.host_row`.
  The host side provides `host_row` via the import-provider mechanism and collects the rows.
  Its expectation is a fixed string.

⁵ **CPython / CRuby executed across backends.**
Both language-runtime binaries are converted *and run*.
Each reads its stdlib from a preopened directory.
`setup.sh` extracts the trees: `cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`.
They are shared per-case consts (`CPYTHON_HELLO`, `CRUBY_HELLO`).
`cpython_hello_e2e!`/`cruby_hello_e2e!` drive them.
The stdlib trees mount straight from the app cache via the case's `cache_preopens` field.
They are never copied.
Each case is ground-truthed against `wasmtime --dir`.
The feature audit reports both as baseline-only (in scope).

No new WASI unit was needed.
The wide import lists below include functions no backend implements:

- CPython imports `poll_oneoff`/`path_link`/`path_symlink`/`path_readlink`/`sock_*`;
- CRuby imports `fd_renumber`/`poll_oneoff`/`path_readlink`.

But none is *called* on the interpreter boot + one-liner path.
So the implemented syscall set suffices, as measured by running to success.

Every backend runs both cases; what varies is the speed category.
Where a case sits at `ultra`, the cost is wall time, not feasibility.
Those are both cases on Go, Java and Bash, plus CRuby on Perl.
The output is correct, so those runs stay out of CI and happen by hand before a release.

Java needed two splitter fixes to run them at all:

- CPython's largest function holds a 3202-target `br_table` as one statement.
  It has no statement boundary to split at.
  Its `switch` alone passes the 64 KB per-method limit (*code too large*).
- CRuby's 8737-entry funcref table saturates one class's 65535-entry constant pool.
  `javac` then reports *too many constants*.

The splitter now cuts a `br_table` at its case ranges.
It also spreads the table's fillers over `ElemF{c}` classes.
Both fixes are general, not app-specific.

Bash needs one thing no other Bash case does: `ulimit -s` raised in the glue.
Every wasm call nests a native bash call.
So CPython's boot otherwise exhausts the 8 MB process stack and dies of SIGSEGV.
The generated standalone entrypoint raises it already; a library-mode embedder has to do it itself.

⁶ **ripgrep.**
ripgrep 14.1.1 built from the pinned source release.
The build uses `cargo build --release --target wasm32-wasip1`.
It uses default features, which already exclude pcre2.
So it needs no tweaks.
Audit: baseline only after the `wasm-opt` pass¹¹, in scope.
The `rg_search` case searches the committed fixture tree `examples/apps/fixtures/rg/` recursively.
The tree is staged into a scratch dir and preopened at `/work`.
The case asserts the guest stdout is byte-identical to the `wasmtime --dir` snapshot.
`--sort path` forces ripgrep's otherwise-parallel walk into a single deterministic order.
Without it the file order varies run-to-run.
ripgrep imports `poll_oneoff`/`path_readlink` but does not call them on this path.
So no new WASI unit was needed.
Every backend runs it against the same fixture and snapshot, `slow_test`-feature-conditional.
Java is the class-split stress case.
rg's ~7300 functions and ~4900-entry function table overflow one class's 65535-entry constant pool.
So its functions are partitioned across five nested `P{k}` classes, each with its own pool.
The table is built in a nested `Elem` class, which also has its own pool.

⁷ **minigzip / zlib (compression CLI).**
zlib 1.3.1's `minigzip` built from the pinned source release.
The build uses `wasi-sdk clang --target=wasm32-wasip1`.
The build compiles the zlib translation units + `test/minigzip.c`.
`-DZ_HAVE_UNISTD_H` makes the shipped `zconf.h` declare `lseek`.
It is integer-only and tiny, with **binary** stdin/stdout.
It is the byte-exact-stdio stress that runs under **every** backend.
The cases are `run_gzip_cases`, invoked via `gzip_e2e!` in each crate.
The compiled backends, Go and Java, prove the byte-stdio path is exact through compiled output too.
There are two cases:

- *compress*: stdin text → gz stdout, byte-identical to the `wasmtime` snapshot.
  The snapshot is `examples/apps/snapshots/minigzip_compress.gz`.
- *round trip*: compress then `-d` decompress → original, self-checking.

zlib's gz stream is deterministic here (mtime 0, OS byte 3).
So wasmtime and every backend agree byte-for-byte.
Not marked slow: no softfloat, so Bash runs it too.
The binary stdin/snapshot cannot travel through the `&str`/`include_str!` `APP_CASES` path.
So these live in a dedicated `run_gzip_cases` that each backend calls.
It uses the bytes-capable `run_bytes`/`run_command_bytes` helpers.

⁸ **libpcap (Track A).**
libpcap 1.10.6 built from the pinned upstream release as a C-API library.
The build uses `wasi-sdk clang --target=wasm32-wasip1 -mexec-model=reactor`.
It compiles only the platform-independent BPF-filter-compilation translation units.
It has no capture backend.
1.10.x no longer ships pre-generated `grammar.c`/`scanner.c`.
So the parser is regenerated with bison/flex.
Audit: baseline only after the `wasm-opt` pass¹¹, in scope.
That pass re-encodes the overlong `call_indirect` immediates.
Our own `examples/apps/src/pcap_binding.c` exports `compile_filter`, which runs `pcap_compile_nopcap`.
It serializes the resulting BPF program into guest memory.
The layout is `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]`.
The C-API case is `pcap_compile` (`pcap_compile_e2e!`).
It drives `compile_filter("tcp port 80", DLT_EN10MB, 65535)` on every backend.
It pins the canonical tcp-port-80 program (deterministic: BPF holds offsets and constants only).
Like the other reactor-library C-API cases it is `slow_test`-conditional.
Bash drives it like the rest.
A guest pointer is a decimal in the `R0` result global.
Guest memory is the module's byte array.
So the walk over the serialized program is plain shell arithmetic.
*Shim caveat:* wasip1 has no `./configure` host, no `socket()`, and no baseline `setjmp`/`longjmp`.
So a first-party `examples/apps/src/pcap_config.h`⁹ stands in for the generated `config.h`.
See its header comment.

⁹ **The `pcap_config.h` shim** collapses three wasip1 gaps:

- the `./configure` feature macros the filter compiler reads;
- placeholders (`socket()`, `SIOCGIF*`) that let the never-reached, wasm-ld-GC'd `pcap_lookupnet` compile;
- a baseline-wasm `setjmp`→0 / `longjmp`→trap stand-in.

libpcap reports filter *syntax errors* via `longjmp`.
Without the out-of-scope wasm exception-handling proposal, wasip1's `<setjmp.h>` refuses to compile.
A valid filter, the only kind this demo compiles, never takes the error path.
So the stand-in is transparent.
An invalid filter would trap rather than return an error.
Name-based filters (`host example.com`) are likewise out of scope.
`pcap_binding.c` stubs the missing `getaddrinfo`/`getnetbyname`/`getprotobyname` to report "not found".

¹⁰ **tree-sitter (Track A).**
The tree-sitter incremental-parsing runtime 0.26.11, as the single-TU amalgamation `lib/src/lib.c`.
It comes with the pre-generated tree-sitter-json 0.24.8 grammar (`src/parser.c`).
Both are built from the pinned upstream releases as a C-API library.
The build uses `wasi-sdk clang -mexec-model=reactor`.
Audit: baseline only after the `wasm-opt` pass¹¹, in scope.
Unlike libpcap, the runtime needs no shim (no `setjmp`, no host lookups).
Our own `examples/apps/src/treesitter_binding.c` exports `parse_source`, which parses a source string.
It returns the parse tree's S-expression into guest memory.
The S-expression comes from `ts_node_string`, a malloc'd C string.
The C-API case is `treesitter_parse` (`treesitter_parse_e2e!`).
It parses the fixed snippet `{"key": [1, true, null]}` on every backend.
It pins this S-expression:

```
(document (object (pair key: (string (string_content)) value: (array (number) (true) (null)))))
```

The S-expression is deterministic: tree-sitter's node naming is fixed by the pinned grammar.
It is `slow_test`-conditional like the other reactor-library C-API cases, Bash included.
Footnote 8 shows how the pointer handling reads in shell.

¹¹ **`wasm-opt` preprocessing.**
Every module `setup.sh` builds from source is run through `wasm-opt -O2` before caching.
The pass uses baseline features only and no ctor-eval.
Those modules are the three sqlite3 shapes, minigzip, libpcap, tree-sitter, and ripgrep.
The DWARF fixture is not among them, since it keeps its debug info.
Besides shrinking them, `wasm-opt` re-encodes the overlong `call_indirect` immediates.
The LLVM toolchain emits those immediates.
So these modules audit as *pure* baseline rather than baseline + the reference-types encoding bit¹.
The downloaded artifacts (qjs, CPython, CRuby) still carry that bit.
cowsay is downloaded but audits as pure baseline too.
Its own release workflow runs the same `wasm-opt` pass before publishing.

¹² **zeroperl retraction (audited 2026-08-01).**
This entry was previously *deferred* on three presumed host-environment blockers.
Converting and running the module proved all three were misreadings, so the verdict is retracted.
(1) **asyncify** and the **setjmp/longjmp** shim are module-internal.
asyncify is a binaryen transform baked into the wasm.
The setjmp implementation is a port of ruby.wasm's `rb_wasm_setjmp`.
It lowers to ordinary baseline instructions.
(2) The imported **`env.call_host_function`** is only invoked when the guest registers a host callback.
The eval path never does that.
So a zero-returning stub as an import provider satisfies the link with no host glue.
(3) No **stdlib preopen** is needed.
The Perl core is embedded in the module as an "SFS" blob served from guest memory.
The only preopen `zeroperl_init` requires is `/dev/null`, mapped guest→host `/dev/null`.
Without it init returns 1.

The prebuilt reactor exposes an embedding C API.
So it is driven exactly like the other reactor-library C-API cases (footnotes ⁸/¹⁰).
The `zeroperl_eval` case (`zeroperl_eval_e2e!`) evaluates a regex + `printf` Perl program.
It pins that program's stdout.
The `6over3/zeroperl` source repo cuts no releases.
So the pinnable distribution is the `@6over3/zeroperl-ts` npm wrapper.
The source is MIT, and the npm wrapper is Apache-2.0.
The `exiftool_extract` case (`exiftool_extract_e2e!`) runs a *real* Perl application.
It runs on the same converted reactor.
The application is the flattened ExifTool 13.42 CLI driver (6over3/exiftool `src/exiftool`).
It is fetched into `cache/exiftool-lib/` and preopened at `/work`.
A committed EXIF image fixture is preopened alongside it at `/img`.
The driver extracts deterministic tags (`-S -Make -Model -DateTimeOriginal`).
The tags are cross-checked against host exiftool.
The case exercises the SFS + preopen path end to end.
`use Image::ExifTool` resolves from the module tree embedded in the SFS blob.
The driver script and image come in through real WASI preopens.
The case also confirms the C-API drive survives ExifTool's terminal `exit`.
That `exit` is overridden to a `die`, so it unwinds into `eval_pv` rather than tripping `proc_exit`.
It reconverts the cached `zeroperl.wasm`, so it adds no convert-suite row.

Both cases run on all six backends, at speeds from `slow` to `ultra`.
*Preopen caveat:* the `/dev/null` preopen is not a directory.
The Python, Go, Java, and Bash runtimes rejected that.
Each now only requires a preopen path to *resolve*, as Ruby and Perl already did.
Python and Bash additionally collapse a final `.` component during path resolution.
They do so because wasi-libc rewrites a path that *is* a preopen to `.`.
`os.open("/dev/null/.")` is then ENOTDIR.
Ruby's `File.realpath`, Perl's `Cwd::realpath`, and Go's `filepath.Join` already collapsed it.
On Java the reactor is what pushed the function-partition threshold down to 2000.
Its ~2450 constant-dense functions overflow a single class's 65535-entry pool.
`javac` then reports *too many constants*.

¹³ **CRuby wasi-vfs-packed (audited 2026-08-04).**
This is ruby.wasm's intended deployment shape.
`setup.sh` packs the two already-pinned CRuby artifacts with the pinned `wasi-vfs` CLI.
The artifacts are `cache/ruby.wasm` plus the `cache/ruby-lib/usr` stdlib tree.
The result is the self-contained `cache/ruby-packed.wasm`.
The official build links `libwasi_vfs.a`.
`wasi-vfs pack` embeds the tree via wizer pre-initialization as ordinary data segments.
So the audit is baseline-only, with the same 37-function import list as the unpacked CRuby.
The case (`CRUBY_PACKED_HELLO`, `cruby_packed_hello_e2e!`) needs **no preopens at all**.
So it is a plain `AppCase`, a stdlib `require "json"` one-liner.
The one-liner proves the embedded VFS serves the tree.
The `wasmtime_test` suite ground-truths it with zero `--dir` flags.
Every backend runs it.
It is faster than the unpacked case wherever both are measured.
That is because the stdlib loads from guest memory instead of host I/O.
But the speed categories still vary by backend.
On Python the constraint is host memory rather than the clock.

¹⁴ **mruby (audited 2026-08-14).**
mruby 3.4.0 built from the pinned source tarball with wasi-sdk clang for wasm32-wasip1.
setjmp/longjmp lowers onto the final exception-handling proposal (`-mllvm -wasm-enable-sjlj`).
So this is the app that exercises `try_table`/`throw` end to end.
Exception handling is accepted input lowered per backend.
The convert manifest asserts each declaring backend converts the module.
It also asserts each non-declaring backend rejects it with the attributed error.
Bash (no exception mechanism) stays on the rejection side.
The execution case is `MRUBY_EH` (`mruby_eh_e2e!`), run on every declaring backend.
It drives raise, rescue, ensure, a custom exception class, and retry.
They run through the converted interpreter.
The wasi build excludes mruby-io, mruby-dir, and mruby-socket.
A first-party `mruby-wasi-puts` gem restores `Kernel#puts`.

¹⁵ **toywasm (audited 2026-08-15).**
The official `wasm32-wasi` release asset of [yamt/toywasm](https://github.com/yamt/toywasm) v76.0.0.
It is `bin/toywasm` out of `toywasm-v76.0.0-wasm32-wasi.tgz`, 909,932 bytes.
It uses the default build configuration, where `--print-build-options` reports six features off.
Three are tail-call, threads, and multi-memory.
The other three are extended-const, custom-page-sizes and exception handling.
The audit confirms the binary itself is baseline wasm with the reference-types encoding bit only¹.
The case is `TOYWASM_COWSAY` (`toywasm_cowsay_e2e!`).
It has the converted interpreter run a *second* wasm binary: the already-cached `cowsay.wasm`.
That binary is loaded from the app cache preopened at `/apps`.
`--wasi` gives that inner guest its own WASI.
Its expected stdout is the `cowsay_args` snapshot.
So the case compares cowsay through the converted interpreter with cowsay directly under wasmtime.
The two must be byte-identical.

That indirect ground truth is the only one available.
wasmtime answers `fd_fdstat_set_flags(0, NONBLOCK)` with EBADF.
It accepts the call on regular files only.
toywasm's WASI setup treats the failure as fatal.
So the pinned binary does not run under wasmtime at all.
dewasm's runtimes accept the call on any open fd and record the flags.
That is why the converted interpreter runs.
The case therefore has no wasmtime freshness run and adds no snapshot file.

¹⁶ **wasm3 (audited 2026-08-29, deferral retracted; re-pinned to v0.9.0 the same day).**
An earlier entry deferred wasm3 on evidence measured against its 2026-08 master snapshot.
That snapshot's whole dispatch is `M3_MUSTTAIL return nextOpImpl()`.
The stock build demands the tail-call proposal.
Its `-DM3_HAS_TAIL_CALL=0` escape was measured growing the C stack once per *executed* opcode.
Issue #101 records both.
The promotion first landed on v0.5.0 (2021-06), which predates that dispatch.
Upstream then resumed releases with v0.9.0 (2026-08-24), and the pin moved there.

Between 2026-08-29 and 2026-08-31 the pin was a local `-DM3_HAS_TAIL_CALL=0` source build.
That was the only way to get a baseline module out of that dispatch.
Once the tail-call proposal became accepted input (decision 88), the pin moved to a release asset.
That asset is the official `wasm3-wasi.wasm`, which is what `scripts/wasm3.sh` fetches now.
The asset is the meta-WASI build (`-Dd_m3HasMetaWASI`).
It forwards the guest's WASI calls straight to the outer host.
That is exactly the shape a converted interpreter needs.
It is also the reason this app takes the guest module directly with no `--wasi` flag.
Audit: baseline plus tail calls, and nothing else.
It carries the reference-types bit for overlong `call_indirect` immediates but uses no construct.

Every backend lowers the proposal, so every backend runs the case.
The old per-opcode stack growth is gone with the source build that caused it.
The asset's dispatch is a tail call.
So each backend's trampoline runs the whole chain in one host frame.
The glue on every backend is now plain.
The wasi-libc compatibility fixes the v0.5.0 build carried as a patch are upstream in v0.9.0.
Its meta-WASI layer serves 38 WASI functions where v0.5.0 served 28.
fd_tell is among the additions: minigzip round-trips through it, measured.

The case (`WASM3_COWSAY`, `wasm3_cowsay_e2e!`) mirrors `TOYWASM_COWSAY`¹⁵.
The converted interpreter runs the cached `cowsay.wasm` out of the app cache preopened at `/apps`.
The case compares against the `cowsay_args` snapshot.
Two differences from toywasm:

- wasm3's CLI takes the guest module directly, with no `--wasi` flag.
  The meta-WASI build always forwards the guest's WASI.
- The artifact runs under wasmtime.
  So the `fs_apps` freshness run cross-checks the case against a live engine.
  That is the ground truth the toywasm case cannot have.

**Why a second interpreter next to toywasm¹⁵.**
wasm3 natively interprets roughly 6x slower than wasmtime, where toywasm sits near 300x.
That makes converted wasm3 the carrier that keeps a converted interpreter competitive.
It competes with the wasm interpreters hand-written in the target languages (wardite, pywasm).
The speed benchmark suite measures that comparison.

## Deferred: pandoc

- Source: [`pandoc.wasm`](https://haskell-wasm.github.io/pandoc-wasm/pandoc.wasm) on the gh-pages of `haskell-wasm/pandoc-wasm`.
  It is unversioned, so record the serving commit when pinning.
  The audited copy is commit `ed18ae6e337d`, 53 MB.
  Its sha256 is `48d9ceed3ef805f6acc28e6f58c2439cdeb1f71864244fffcc155e2c045aa7fc`.
- Audit: **needs simd** (first offense: a v128 operation at offset 0x24723a).
  Notably it does *not* need tail calls or exception handling.
  The GHC 9.12 wasm backend output is otherwise baseline-shaped.
  So SIMD support alone would unblock it.
- Revisit when/if SIMD enters scope.
  The binary is otherwise a pure wasip1 stdio converter and would make a strong demo.

## Deferred: LightningCSS

- Source: [github.com/pgaskin/go-lightningcss](https://github.com/pgaskin/go-lightningcss), a Rust **reactor** build of LightningCSS.
  LightningCSS is the CSS parser/transformer.
  The build was produced via a **pgaskin/wasm2go fork** of the build tooling.
  The published artifact is therefore unverified against an upstream release.
- Audit: **not yet run**, deferred pending audit.
  The fork-built artifact is not trustworthy enough to promote as-is.
- Revisit by pinning the build: a reproducible from-source recipe, not the fork's prebuilt wasm.
  Then run the feature-audit on the resulting binary before promoting it in scope.

## Deferred: Lua

- Source: no viable candidate.
  [nalgeon/lua-wasi](https://github.com/nalgeon/lua-wasi) was archived by its owner in 2026-02.
  Its npm package is `@antonz/lua-wasi`: MIT, wasm32-wasip1, 329 KB.
  It runs a plain script under `wasmtime`.
  But `pcall`/`error`, Lua's core error primitive, crash the whole VM with a wasm `unreachable` trap.
  Its own README concedes the cause: setjmp/longjmp are stubbed out to do nothing.
  [singlestore-labs/lua-wasi](https://github.com/singlestore-labs/lua-wasi) carries the same disclaimer.
  `vvanders/wasm_lua` and `ceifa/wasmoon` are Emscripten/browser-JS builds, not standalone WASI.
  VMware's webassembly-language-runtimes project has no Lua build at all.
- Audit: **no working artifact exists, and the from-source build fails in the linker.**
  Lua 5.4.7 was built from source with `zig cc -target wasm32-wasi`.
  The build stops at "Setjmp/longjmp support requires Exception handling support".
  Adding `-mllvm -wasm-enable-sjlj` gets past it, the same lowering mruby's build¹⁴ uses.
  At that point wasm-ld crashes on a SjLj-plus-weak-symbol bug (clang 21, measured 2026-08-02).
  The build toolchain has since moved to wasi-sdk.
  So a retry would go through wasi-sdk clang and its shipped libsetjmp.
  The exception-handling requirement itself is no longer a blocker.
  Since the mruby work¹⁴ it is accepted input, lowered per backend.
  Added value is low regardless.
  A Lua build would cover the same category (a complete small scripting engine in C) as QuickJS.
  It would also cover the same WASI surface QuickJS already gives.
- Revisit if the wasm-ld bug gets fixed upstream.
  The mruby recipe¹⁴ should then apply directly.
  Also revisit if a maintained WASI build with a working `pcall` appears.

## Deferred: PHP

- Source: no maintained WASI build exists.
  VMware WLR's is the only prebuilt one, `php/8.2.6+20230714`, three years stale.
  Its own writeup states it strips setjmp/longjmp, breaking exceptions and fatal-error handling.
  It also strips all networking and no-ops many filesystem/process syscalls.
  `php/php-src` carries an experimental wasm32-wasi target from a Jan-2023 RFC.
  Its setjmp/longjmp emulation and Fibers are still WIP, and it has no maintained release artifact.
  seanmorris/php-wasm and WordPress Playground's `php.wasm` are Emscripten plus JS glue.
  Neither is a standalone WASI module.
- Audit: **no artifact to audit, and no realistic build path.**
  PHP's Zend engine uses `zend_try`/`zend_catch` for essentially all error and exception control flow.
  These are setjmp-based and pervasive, not opt-in the way Lua's `pcall` is.
  The only prebuilt strips exactly that machinery.
  A from-source build would be a multi-week port, not a build-flag fix like mruby's¹⁴.
- Revisit only if upstream `php-src` ships a maintained wasm32-wasi target.
  That target must have working exception handling.

## WASI p1 import surfaces

The audit also prints each binary's imported WASI functions; the widest candidates are:

- **CPython**: 42 functions, the full p1 surface.
  It includes `fd_pread`/`fd_pwrite`/`fd_tell`/`fd_advise`/`fd_datasync`.
  It also includes `path_link`/`path_rename`/`path_symlink`, `sched_yield`, and the four `sock_*` functions.
- **CRuby**: 37 functions.
  They are CPython's list minus the `sock_*` family, `sched_yield`, and `fd_filestat_set_times`.
  `fd_renumber` is added.

Importing is not calling: the out-of-scope `sock_*` imports still resolve to the ENOSYS stub.
But a runtime implementation is not required for scripts that never open sockets.
Running both to success confirmed this (footnote ⁵).
Both runtimes read their stdlib trees from a preopened directory at startup.
`setup.sh` now extracts those trees, and the e2e cases preopen them:

- `cache/cpython-lib/lib/python3.14` at guest `/lib`;
- `cache/ruby-lib/usr/local/lib/ruby` at guest `/usr`.

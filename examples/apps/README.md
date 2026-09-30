# Real-world example apps

Wasm binaries of real applications, built in advance, for examples and end-to-end tests.
Each is fetched from its own upstream.

**No third-party artifact is committed to this repository.**
`./setup.sh` fetches files at fixed versions, verified by SHA-256, into `cache/`, which Git ignores.
Licensing of the binaries stays entirely with their upstream distribution.
The `apps` cases convert each cached app and compare its output with the snapshot files in `snapshots/`.
They live in `crates/dewasm-test-helper/src/apps.rs`.
`apps_capi.rs` and `apps_fs.rs` beside it hold the C-API and file system shapes.
The snapshots were captured once from Wasmtime, and `--features wasmtime_test` re-validates them.
Each backend runs the cases as its `e2e` test, e.g. `cargo test -p dewasm-backend-ruby --test e2e apps`.
A missing cache or `ruby` fails the test loudly rather than skipping.
Run `./setup.sh` first.

`setup.sh` runs the per-app scripts in `scripts/`, with shared code in `scripts/common.sh`.
Run one directly (e.g. `scripts/sqlite3.sh`) to rebuild a single app after changing its fixed version.

| App | Source | What it demonstrates |
| --- | --- | --- |
| `cowsay` | [dewasm/cowsay.wasm v0.2.0](https://github.com/dewasm/cowsay.wasm/releases/tag/v0.2.0), our own C reimplementation of `cowsay` 3.03 | arguments + `stdout`, the classic example |
| `qjs` | [QuickJS-ng v0.15.1](https://github.com/quickjs-ng/quickjs/releases/tag/v0.15.1) `qjs-wasi.wasm` (official WASI CLI release asset) | a complete JavaScript engine (1.5 MB wasm) running on plain Ruby; deepened with file-I/O and REPL fixtures (Phase 5a) |
| `sqlite3` | [SQLite 3.53.3 amalgamation](https://sqlite.org/2026/sqlite-amalgamation-3530300.zip), built from source with `wasi-sdk` | the full SQLite engine in four shapes: the CLI shell, the same shell with the hot VDBE opcode bodies split into their own functions (`src/sqlite3-vdbe-split.patch`, measured by the `app/sqlite3_mod_query` benchmark), the C-API library, and a guest→host callback binding; the build patches a `-wasm` suffix into the reported version (`3.53.3-wasm`) so converted output identifies itself |
| `minigzip` | [`zlib` 1.3.1](https://github.com/madler/zlib/releases/tag/v1.3.1), built from source with `wasi-sdk` | compression and decompression of binary data on standard input and output, the `gzip` stress test with identical output bytes that runs under **all five backends** (Phase 5b) |
| `rg` (`ripgrep`) | [`ripgrep` 14.1.1](https://github.com/BurntSushi/ripgrep/releases/tag/14.1.1), built with `cargo build --target wasm32-wasip1` | recursive directory search over a preopened fixture tree, byte-identical to `wasmtime --dir` (Ruby + Python + Go + Java, Phase 5b) |
| `cpython` | [CPython 3.14.6 WASI build](https://github.com/brettcannon/cpython-wasi-build/releases/tag/v3.14.6) (unofficial, a WASI build made in advance by a core developer) | a whole Python interpreter converted and **executed**, reading its standard library from a preopen (Ruby + Python + Go, heavy; Phase 5b) |
| `ruby` (CRuby) | [ruby.wasm 2.9.4](https://github.com/ruby/ruby.wasm/releases/tag/2.9.4) full build (official build made in advance) | CRuby 3.4 executed on the Ruby backend, the "Ruby on Ruby" goal example (Ruby + Python, heavy; Phase 5b) |
| `libpcap` | [`libpcap` 1.10.6](https://www.tcpdump.org/release/libpcap-1.10.6.tar.gz), built from source with `wasi-sdk` (reactor) | the BPF filter compiler of `libpcap` as a C-API library: `compile_filter("tcp port 80")` returns the bytes of a BPF program from guest memory, driven on Ruby + Python + Go (heavy) |
| `treesitter` | [tree-sitter 0.26.11](https://github.com/tree-sitter/tree-sitter/releases/tag/v0.26.11) + [`tree-sitter-json` 0.24.8](https://github.com/tree-sitter/tree-sitter-json/releases/tag/v0.24.8), built from source with `wasi-sdk` (reactor) | the tree-sitter parsing runtime as a C-API library: `parse_source("{...}")` returns the JSON parse tree's S-expression, driven on Ruby + Python + Go (heavy) |
| `zeroperl` | [`6over3/zeroperl`](https://github.com/6over3/zeroperl) (Perl 5.42), wasm built in advance from the [`@6over3/zeroperl-ts`](https://www.npmjs.com/package/@6over3/zeroperl-ts) `npm` package | the Perl 5 interpreter (25 MB reactor) driven through its embedding C API: `zeroperl_eval` runs a Perl program and prints its output on Ruby (heavy) |
| `exiftool` | [`6over3/exiftool`](https://github.com/6over3/exiftool) `src/exiftool` (ExifTool 13.42, Phil Harvey's `Image::ExifTool` written only in Perl, flattened), the driver script only, into `cache/exiftool-lib/`; runs on the cached `zeroperl.wasm` | a real Perl app: the ExifTool CLI extracts EXIF tags from a committed image fixture through the converted Perl reactor, exercising the SFS-embedded module tree + preopened script/image (Ruby, heavy; no new wasm) |
| `toywasm` | [`toywasm` v76.0.0](https://github.com/yamt/toywasm/releases/tag/v76.0.0) `bin/toywasm` from the official `wasm32-wasi` release asset | a WebAssembly interpreter, itself compiled to wasm, converted to each backend and then interpreting a second cached wasm binary: `cowsay` run through the converted interpreter prints the same bytes as `cowsay` run directly (every backend) |
| `mruby` | [`mruby` 3.4.0](https://github.com/mruby/mruby/archive/refs/tags/3.4.0.tar.gz), built from source with `wasi-sdk` and the `rake` of `mruby` | the `mruby` CLI, compiled with LLVM's SJLJ lowering so raise/rescue/ensure become wasm exception-handling instructions (`try_table`/`throw`); a fixture for the in-progress wasm exception-handling proposal support, not yet driven by any backend |

```console
$ ./setup.sh
$ cargo run -q -p dewasm -- examples/apps/cache/qjs.wasm --target ruby --mode standalone -o qjs.rb
$ ruby qjs.rb -e 'console.log("JS on Ruby:", 6 * 7)'
JS on Ruby: 42
```

Candidates need only the implemented WASI surface (see [`docs/support.md`](../../docs/support.md)).
With WASI file system support now landed for Ruby, that includes real file-backed I/O.
It is not limited to standard I/O, arguments, environment variables, clocks, and random numbers.

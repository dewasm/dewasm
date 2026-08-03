# Real-world example apps

Prebuilt wasm binaries of real applications, each fetched from its own
upstream, for demos and end-to-end tests.

**No third-party artifact is committed to this repository** (ADR-9).
`./setup.sh` downloads version-pinned, sha256-verified files into
`cache/` (gitignored); licensing of the binaries stays entirely with
their upstream distribution. The `apps` cases
(`crates/dewasm-test-helper/src/apps.rs`, plus `apps_capi.rs` and
`apps_fs.rs` for the C-API and filesystem shapes) convert each cached app
and compare its output against the snapshot files in `snapshots/` (captured
once from wasmtime; re-validated via `--features wasmtime_test`), run per
backend as that backend's `e2e` test, e.g.
`cargo test -p dewasm-backend-ruby --test e2e apps`. A missing cache or
`ruby` fails the test loudly rather than skipping (ADR-15) — run
`./setup.sh` first.

`setup.sh` just runs the per-app scripts in `scripts/` (shared
boilerplate in `scripts/common.sh`); run one directly — e.g.
`scripts/sqlite3.sh` — to rebuild a single app after bumping its pin.

| App | Source | What it demonstrates |
| --- | --- | --- |
| cowsay | [syrusakbary/cowsay@0.3.0](https://wasmer.io/syrusakbary/cowsay) (Wasmer registry) | args + stdout, the classic demo |
| qjs | [quickjs-ng v0.15.1](https://github.com/quickjs-ng/quickjs/releases/tag/v0.15.1) `qjs-wasi.wasm` (official WASI CLI release asset) | a complete JavaScript engine (1.5 MB wasm) running on plain Ruby; deepened with file-I/O and REPL fixtures (Phase 5a) |
| sqlite3 | [sqlite 3.53.3 amalgamation](https://sqlite.org/2026/sqlite-amalgamation-3530300.zip), built from source with `zig` (ADR-22) | the full SQLite engine in three shapes: the CLI shell, the C-API library, and a guest→host callback binding |
| minigzip | [zlib 1.3.1](https://github.com/madler/zlib/releases/tag/v1.3.1), built from source with `zig` | binary-stdio (de)compression — the byte-exact gzip stress that runs under **all five backends** (Phase 5b) |
| rg (ripgrep) | [ripgrep 14.1.1](https://github.com/BurntSushi/ripgrep/releases/tag/14.1.1), built with `cargo build --target wasm32-wasip1` | recursive directory search over a preopened fixture tree, byte-identical to `wasmtime --dir` (Ruby + Python + Go + Java, Phase 5b) |
| cpython | [CPython 3.14.6 wasi build](https://github.com/brettcannon/cpython-wasi-build/releases/tag/v3.14.6) (unofficial prebuilt — a core dev's WASI build) | a whole Python interpreter converted and **executed**, reading its stdlib from a preopen (Ruby + Python + Go, heavy; Phase 5b) |
| ruby (CRuby) | [ruby.wasm 2.9.4](https://github.com/ruby/ruby.wasm/releases/tag/2.9.4) full build (official prebuilt) | CRuby 3.4 executed on the Ruby backend — the "Ruby on Ruby" north-star demo (Ruby + Python, heavy; Phase 5b) |
| libpcap | [libpcap 1.10.6](https://www.tcpdump.org/release/libpcap-1.10.6.tar.gz), built from source with `zig` (reactor) | libpcap's BPF filter compiler as a C-API library: `compile_filter("tcp port 80")` returns a serialized BPF program from guest memory, driven on Ruby + Python + Go (heavy) |
| treesitter | [tree-sitter 0.26.11](https://github.com/tree-sitter/tree-sitter/releases/tag/v0.26.11) + [tree-sitter-json 0.24.8](https://github.com/tree-sitter/tree-sitter-json/releases/tag/v0.24.8), built from source with `zig` (reactor) | the tree-sitter parsing runtime as a C-API library: `parse_source("{...}")` returns the JSON parse tree's S-expression, driven on Ruby + Python + Go (heavy) |
| zeroperl | [6over3/zeroperl](https://github.com/6over3/zeroperl) (Perl 5.42), prebuilt wasm from the [`@6over3/zeroperl-ts`](https://www.npmjs.com/package/@6over3/zeroperl-ts) npm package | the Perl 5 interpreter (25 MB reactor) driven through its embedding C API: `zeroperl_eval` runs a Perl program and prints its output on Ruby (heavy) |
| exiftool | [6over3/exiftool](https://github.com/6over3/exiftool) `src/exiftool` (ExifTool 13.42, Phil Harvey's pure-Perl `Image::ExifTool`, flattened) — the driver script only, into `cache/exiftool-lib/`; runs on the cached `zeroperl.wasm` | a real Perl app: the ExifTool CLI extracts EXIF tags from a committed image fixture through the converted Perl reactor, exercising the SFS-embedded module tree + preopened script/image (Ruby, heavy; no new wasm) |

```console
$ ./setup.sh
$ cargo run -q -p dewasm-cli -- examples/apps/cache/qjs.wasm --mode standalone -o qjs.rb
$ ruby qjs.rb -e 'console.log("JS on Ruby:", 6 * 7)'
JS on Ruby: 42
```

Candidates need only the implemented WASI surface (see
[docs/support.md](../../docs/support.md)) — with WASI filesystem support
now landed for Ruby (ADR-14), that includes real file-backed I/O, not
just stdio/args/environ/clocks/random.

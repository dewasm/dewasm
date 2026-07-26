# Real-world example apps

Prebuilt wasm binaries of real applications, each fetched from its own
upstream, for demos and end-to-end tests.

**No third-party artifact is committed to this repository** (ADR-9).
`./fetch.sh` downloads version-pinned, sha256-verified files into
`cache/` (gitignored); licensing of the binaries stays entirely with
their upstream distribution. The `apps` cases of the `e2e` test
(`crates/dewasm-cli/tests/e2e/apps.rs`) convert each cached app and
compare its output against the golden files in `golden/` (captured once
from wasmtime; re-validated via `--features wasmtime_test`). A missing
cache or `ruby` fails the test loudly rather than skipping (ADR-15) —
run `./fetch.sh` first.

| App | Source | What it demonstrates |
| --- | --- | --- |
| cowsay | [syrusakbary/cowsay@0.3.0](https://wasmer.io/syrusakbary/cowsay) (Wasmer registry) | args + stdout, the classic demo |
| qjs | [quickjs-ng v0.15.1](https://github.com/quickjs-ng/quickjs/releases/tag/v0.15.1) `qjs-wasi.wasm` (official WASI CLI release asset) | a complete JavaScript engine (1.5 MB wasm) running on plain Ruby; deepened with file-I/O and REPL fixtures (Phase 5a) |
| sqlite3 | [sqlite 3.53.3 amalgamation](https://sqlite.org/2026/sqlite-amalgamation-3530300.zip), built from source with `zig` (ADR-22) | the full SQLite engine in three shapes: the CLI shell, the C-API library, and a guest→host callback binding |
| minigzip | [zlib 1.3.1](https://github.com/madler/zlib/releases/tag/v1.3.1), built from source with `zig` | binary-stdio (de)compression — the byte-exact gzip stress that runs under **both** Ruby and Bash (Phase 5b) |
| rg (ripgrep) | [ripgrep 14.1.1](https://github.com/BurntSushi/ripgrep/releases/tag/14.1.1), built with `cargo build --target wasm32-wasip1` | recursive directory search over a preopened fixture tree, byte-identical to `wasmtime --dir` (Ruby-only, Phase 5b) |
| cpython | [CPython 3.14.6 wasi build](https://github.com/brettcannon/cpython-wasi-build/releases/tag/v3.14.6) (official prebuilt) | a whole Python interpreter converted and **executed** on Ruby, reading its stdlib from a preopen (Ruby-only, heavy; Phase 5b) |
| ruby (CRuby) | [ruby.wasm 2.9.4](https://github.com/ruby/ruby.wasm/releases/tag/2.9.4) full build (official prebuilt) | CRuby 3.4 executed on the Ruby backend — the "Ruby on Ruby" north-star demo (Ruby-only, heavy; Phase 5b) |

```console
$ ./fetch.sh
$ cargo run -q -p dewasm-cli -- examples/apps/cache/qjs.wasm --mode standalone -o qjs.rb
$ ruby qjs.rb -e 'console.log("JS on Ruby:", 6 * 7)'
JS on Ruby: 42
```

Candidates need only the implemented WASI surface (see
[docs/support.md](../../docs/support.md)) — with WASI filesystem support
now landed for Ruby (ADR-14), that includes real file-backed I/O, not
just stdio/args/environ/clocks/random.

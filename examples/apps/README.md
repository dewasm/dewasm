# Real-world example apps

Prebuilt wasm binaries of real applications, each fetched from its own
upstream, for demos and end-to-end tests.

**No third-party artifact is committed to this repository** (ADR-9).
`./fetch.sh` downloads version-pinned, sha256-verified files into
`cache/` (gitignored); licensing of the binaries stays entirely with
their upstream distribution. The `apps` cases of the `e2e` test
(`crates/dewasmify-cli/tests/e2e/apps.rs`) convert each cached app and
compare its output against wasmtime; they self-skip when the cache,
`ruby`, or `wasmtime` is missing.

| App | Source | What it demonstrates |
| --- | --- | --- |
| cowsay | [syrusakbary/cowsay@0.3.0](https://wasmer.io/syrusakbary/cowsay) (Wasmer registry) | args + stdout, the classic demo |
| qjs | [quickjs-ng v0.15.1](https://github.com/quickjs-ng/quickjs/releases/tag/v0.15.1) `qjs-wasi.wasm` (official WASI CLI release asset) | a complete JavaScript engine (1.5 MB wasm) running on plain Ruby |
| sqlite | [sqlite@0.2.2](https://wasmer.io/sqlite) (Wasmer registry) | the full SQLite engine (in-memory databases via the CLI shell; the stepping stone toward a library-mode sqlite3 driver). sqlite.org's own "WebAssembly" download is an Emscripten browser/Node.js bundle, not a standalone WASI binary, so this stays on the registry build |

```console
$ ./fetch.sh
$ cargo run -q -p dewasmify-cli -- examples/apps/cache/qjs.wasm --mode standalone -o qjs.rb
$ ruby qjs.rb -e 'console.log("JS on Ruby:", 6 * 7)'
JS on Ruby: 42
```

Candidates need only the implemented WASI surface (see
[docs/support.md](../../docs/support.md)) — with WASI filesystem support
now landed for Ruby (ADR-14), that includes real file-backed I/O, not
just stdio/args/environ/clocks/random.

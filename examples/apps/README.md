# Real-world example apps

Prebuilt wasm binaries of real applications, fetched from the
[Wasmer registry](https://wasmer.io/) for demos and end-to-end tests.

**No third-party artifact is committed to this repository** (ADR-9).
`./fetch.sh` downloads version-pinned, sha256-verified packages into
`cache/` (gitignored); licensing of the binaries stays entirely with
their upstream distribution. The `apps` e2e test
(`crates/dewasmify-cli/tests/apps.rs`) converts each cached app and
compares its output against wasmtime; it self-skips when the cache,
`ruby`, or `wasmtime` is missing.

| App | Package | What it demonstrates |
| --- | --- | --- |
| cowsay | [syrusakbary/cowsay@0.3.0](https://wasmer.io/syrusakbary/cowsay) | args + stdout, the classic demo |
| qjs | [quickjs@0.0.3](https://wasmer.io/quickjs) | a complete JavaScript engine (2.6 MB wasm, C via wasi-sdk, `wasi_unstable`) running on plain Ruby |

```console
$ ./fetch.sh
$ cargo run -q -p dewasmify-cli -- examples/apps/cache/qjs.wasm --mode standalone -o qjs.rb
$ ruby qjs.rb -e 'console.log("JS on Ruby:", 6 * 7)'
JS on Ruby: 42
```

Candidates need only the implemented WASI surface (see
[docs/support.md](../../docs/support.md)): stdio, args, environ, clocks,
random. Anything needing `path_open` waits for WASI filesystem support.

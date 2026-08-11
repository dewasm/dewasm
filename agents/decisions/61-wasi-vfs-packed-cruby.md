# Decision 61: Cover ruby.wasm's wasi-vfs-Packed Shape by Packing In-Cache

Status: **Accepted, 2026-08-04.**
`examples/apps/scripts/cruby.sh` packs the already-cached CRuby with the `wasi-vfs` CLI into `cache/ruby-packed.wasm`; the case runs on Ruby/Python/Perl and converts everywhere (issue #123).

## Context

ruby.wasm is designed to be deployed with [wasi-vfs](https://github.com/kateinoigakukun/wasi-vfs): the official prebuilt modules link `libwasi_vfs.a`, and the intended shape (`rbwasm pack`, a wasi-vfs wrapper) embeds the stdlib and app files into the module via wizer pre-initialization, yielding a self-contained wasm that needs no preopens.
Our `CRUBY_HELLO` case (decision 27, `crates/dewasm-test-helper/src/apps_fs.rs`) covers only the unpacked shape (stdlib served from a `/usr` preopen), so ruby.wasm's primary real-world usage had no coverage.
A packed module is still an ordinary WASI command module (wasi-vfs shadows the `wasi_snapshot_preview1` imports at link time; wizer materializes the loaded files as data segments), and conversion needed no code change: the gap was purely test coverage, plus the fact that wizer-scale data segments are a converter input shape nothing else in the cache exercises.

## Decision

`setup.sh` derives the packed artifact **in-cache from the already-pinned inputs**: `examples/apps/scripts/cruby.sh` runs `wasi-vfs pack cache/ruby.wasm --dir cache/ruby-lib/usr::/usr -o cache/ruby-packed.wasm`, with the `wasi-vfs` CLI required on PATH like the other build tools (decision 15) and the stamp folding the ruby archive sha plus `wasi-vfs --version` (the wasm-opt discipline, decision 39).
CI installs the pinned CLI release and folds its version into the apps-cache key.

The reusable criterion: **when a deployment shape is derivable from inputs the cache already pins by a pinnable tool, derive it in-cache rather than pinning a second upstream artifact**.
One download of the bytes, one version to bump, and the derivation itself (here: that packing works on the official build at all) is under test.

Because the packed module needs no preopens, the case (`CRUBY_PACKED_HELLO`, `crates/dewasm-test-helper/src/apps.rs`) is a plain `AppCase` (a stdlib `require` one-liner proving the embedded VFS serves the tree), where the unpacked CRuby is an `FsAppCase`; expected stdout is inline (the interpreter-hello convention), revalidated against a live engine by the `wasmtime_test` suite.

## Rejected alternatives

- **Consume an upstream pre-packed artifact** (e.g. the packed module inside the `@ruby/*-wasm-wasi` npm packages).
  Pins a second multi-MB download duplicating the interpreter+stdlib bytes we already pin, adds a second distribution channel (npm), and leaves the pack step itself (the thing this decision wants covered) outside the test.
- **No coverage (status quo).**
  Leaves ruby.wasm's intended usage untested and the wizer data-segment shape unexercised; the whole point of the app suite is the shapes users actually run (decision 9).
- **Packing CPython the same way.**
  wasi-vfs can only pack modules linked against `libwasi_vfs.a`; the pinned `brettcannon/cpython-wasi-build` binary is not, so this would mean building CPython from source with the library linked in: rebuilding an interpreter we deliberately consume prebuilt is out of proportion to the coverage gained (decided against in issue #123).

## Consequences

- The intended ruby.wasm deployment shape is covered end-to-end: pack (setup.sh) → convert (a `heavy` row in the whole-cache convert manifest, decision 54) → run (`cruby_packed_hello_e2e!` on Ruby at `slow`, Python/Perl/Bash at ultra, Python demoted by issue #126 (a CI-runner memory limit), Bash wired with its other giants in issue #143; Go and Java excluded for the unpacked CRuby's own reasons: the packed module is the same interpreter, strictly larger).
- `setup.sh` gains a required tool: `wasi-vfs` (prebuilt CLI or `cargo install wasi-vfs-cli`; `require_tool` fails loudly without it).
  Its version participates in the stamp and the CI cache key, so a CLI bump re-packs.
- The cache grows by the ~49 MB packed module; the convert suites pay one more heavy trial per backend.

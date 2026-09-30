# Decision 61: Cover ruby.wasm's wasi-vfs-Packed Shape by Packing In-Cache

Status: **Accepted, 2026-08-04.**
`examples/apps/scripts/cruby.sh` packs the already-cached CRuby with the `wasi-vfs` CLI.
The output is `cache/ruby-packed.wasm`.
The case runs on Ruby/Python/Perl and converts everywhere (issue #123).

## Context

ruby.wasm is designed to be deployed with [wasi-vfs](https://github.com/kateinoigakukun/wasi-vfs).
The official prebuilt modules link `libwasi_vfs.a`.
The intended shape is `rbwasm pack`, a wasi-vfs wrapper.
It embeds the stdlib and app files into the module via wizer pre-initialization.
That yields a self-contained wasm that needs no preopens.
Our `CRUBY_HELLO` case (decision 27, `crates/dewasm-test-helper/src/apps_fs.rs`) covers only one shape.
That is the unpacked shape (stdlib served from a `/usr` preopen).
So ruby.wasm's primary real-world usage had no coverage.

A packed module is still an ordinary WASI command module:

- wasi-vfs shadows the `wasi_snapshot_preview1` imports at link time;
- wizer materializes the loaded files as data segments.

Conversion needed no code change: the gap was purely test coverage.
Wizer-scale data segments are also a converter input shape nothing else in the cache exercises.

## Decision

`setup.sh` derives the packed artifact **in-cache from the already-pinned inputs**.
`examples/apps/scripts/cruby.sh` runs this:

```sh
wasi-vfs pack cache/ruby.wasm --dir cache/ruby-lib/usr::/usr -o cache/ruby-packed.wasm
```

The `wasi-vfs` CLI is required on PATH like the other build tools (decision 15).
The stamp folds the ruby archive sha plus `wasi-vfs --version` (the wasm-opt discipline, decision 39).
CI installs the pinned CLI release and folds its version into the apps-cache key.

The reusable criterion: **derive a deployment shape in-cache; do not pin a second upstream artifact**.
It applies **when a pinnable tool can derive the shape from inputs the cache already pins**.
That means one download of the bytes and one version to bump.
The derivation itself is also under test (here: that packing works on the official build at all).

Because the packed module needs no preopens, the case is a plain `AppCase`.
The case is `CRUBY_PACKED_HELLO` in `crates/dewasm-test-helper/src/apps.rs`.
It is a stdlib `require` one-liner proving the embedded VFS serves the tree.
The unpacked CRuby, by contrast, is an `FsAppCase`.
Expected stdout is inline (the interpreter-hello convention).
The `wasmtime_test` suite revalidates it against a live engine.

## Rejected alternatives

- **Consume an upstream pre-packed artifact.**
  An example is the packed module inside the `@ruby/*-wasm-wasi` npm packages.
  This pins a second multi-MB download duplicating the interpreter+stdlib bytes we already pin.
  It adds a second distribution channel (npm).
  It leaves the pack step itself (the thing this decision wants covered) outside the test.
- **No coverage (status quo).**
  Leaves ruby.wasm's intended usage untested and the wizer data-segment shape unexercised.
  The whole point of the app suite is the shapes users actually run (decision 9).
- **Packing CPython the same way.**
  wasi-vfs can only pack modules linked against `libwasi_vfs.a`.
  The pinned `brettcannon/cpython-wasi-build` binary is not.
  So this would mean building CPython from source with the library linked in.
  That rebuilds an interpreter we deliberately consume prebuilt.
  It is out of proportion to the coverage gained (decided against in issue #123).

## Consequences

- The intended ruby.wasm deployment shape is covered end-to-end:
  - pack: `setup.sh`;
  - convert: a `heavy` row in the whole-cache convert manifest (decision 54);
  - run: `cruby_packed_hello_e2e!`.

  The run is on Ruby at `slow`, and on Python/Perl/Bash at ultra.
  Python was demoted by issue #126 (a CI-runner memory limit).
  Bash was added with its other giants in issue #143.
  Go and Java are excluded for the unpacked CRuby's own reasons.
  The packed module is the same interpreter, strictly larger.
- `setup.sh` gains a required tool: `wasi-vfs`.
  It is a prebuilt CLI or `cargo install wasi-vfs-cli`; `require_tool` fails loudly without it.
  Its version participates in the stamp and the CI cache key, so a CLI bump re-packs.
- The cache grows by the ~49 MB packed module.
  The convert suites pay one more heavy trial per backend.

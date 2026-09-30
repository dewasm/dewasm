# Decision 95: cowsay Comes From Our Own Implementation, Published Upstream

Status: **Accepted, 2026-09-18.**
Implemented: [`examples/apps/scripts/cowsay.sh`](../../examples/apps/scripts/cowsay.sh) pins [dewasm/cowsay.wasm](https://github.com/dewasm/cowsay.wasm) v0.1.0.
That is a C reimplementation of cowsay 3.03.
Its output is byte-identical to the original Perl script.
The fetch-and-pin mechanism of decision 9 is unchanged; only the upstream this app points at is.

## Context

The showpiece app was the Wasmer registry's cowsay 0.3.0, a Rust clone.
It made a poor showpiece on both counts a showpiece is judged by.
Its cow was drawn wrong: the legs line sat one column off the body.
The balloon also dropped the trailing space the original emits.
So the snapshots this repository checked in were of visibly broken art.
And it weighed 772 kB for a program that prints a cow, which the backends then multiply.
The Bash conversion was 7.73 MB.
That is too large to open and read as the "look at the generated source" demo it exists to be.

## Decision

An app's upstream can be *wrong for the demo* while the app is small enough to reimplement.
Then write and publish the implementation.
Do not pin a better-behaved third party, and do not commit sources here.
The criterion is *upstream stays upstream* (decision 9).
This repository holds a URL and a hash.
A separate repository owns the implementation, its tests, and its releases.
Our own upstream is still an upstream, so nothing about the fetch, the pin, or the audit changes.

The published implementation carries its own correctness argument.
A differential test suite runs it against the vendored cowsay 3.03 under the host Perl.
The suite compares stdout, stderr and the exit code over 738 cases.
So "byte-identical to the original" is enforced there and not assumed here.

## Rejected alternatives

- **Keep the registry build.**
  The README and every backend's snapshot would keep showing the broken art.
  The size also defeats the demo.
- **Commit the C sources here.**
  Decision 9 keeps app artifacts out of this repository.
  This cowsay implementation has its own Perl-differential suite.
  Such a project has no business inside a wasm converter's tree.
- **Build it from source in `setup.sh`, as sqlite3 and minigzip are (decision 92).**
  Those are third-party sources we must build, because nobody publishes a WASI binary.
  For our own code we control the release.
  So a pinned release asset costs contributors nothing, and keeps the bytes identical everywhere.
- **Pin another existing cowsay port.**
  Every one surveyed either draws the same broken art or carries a language runtime of its own.
  That is the size problem again.

## Consequences

- Positive: the demo shows the cow the original draws.
  The Bash conversion drops from 7.73 MB to 1.13 MB, small enough to read.
  The module audits as pure baseline (`agents/apps-audit.md`).
  That is because its release runs the same `wasm-opt` pass the locally-built modules get.
- Positive: the app now exercises the WASI filesystem surface in the default `cargo test` run.
  That is because cowfile lookup honours `COWPATH`.
  Its imports go from 8 functions (stdio, args, environ, `random_get`, `proc_exit`) to 17.
  The added ones are `path_open`, `fd_readdir`, `path_filestat_get` and the preopen calls.
- Negative: `app/cowsay` rows are not comparable across this change.
  A speed or size record from before it cannot be compared row by row against a later one.
  That is because the module is a different program.
  The records are dated snapshots, so the break is visible rather than silent.
- Carry-over: some cases follow the pinned release:
  - the cowsay snapshots under `examples/apps/snapshots/`;
  - the two interpreter cases that run cowsay as an inner guest (`TOYWASM_COWSAY`, `WASM3_COWSAY`).

  Bumping the pin means regenerating them with `cargo xtask update-snapshots cowsay`.

See also:
- [decision 9](9-example-apps-from-registry.md) (the fetch-and-pin policy this stays inside);
- [decision 39](39-wasm-opt-preprocessing.md) (the wasm-opt pass the release mirrors);
- [decision 92](92-wasi-sdk-c-toolchain.md) (the C toolchain both use).

# Decision 95: `cowsay` Comes From Our Own Implementation, Published Upstream

Status: **Accepted, 2026-09-18.**
Implemented: [`examples/apps/scripts/cowsay.sh`](../../examples/apps/scripts/cowsay.sh) fixes [dewasm/cowsay.wasm](https://github.com/dewasm/cowsay.wasm) at v0.1.0.
That is a C reimplementation of `cowsay` 3.03.
Its output is byte-identical to the original Perl script.
The fetch at a fixed version (decision 9) is unchanged; only the upstream this app points at is.

## Context

The app shown to readers was the Wasmer registry's `cowsay` 0.3.0, a Rust clone.
It made a poor app to show, on both counts such an app is judged by.
Its cow was drawn wrong: the legs line sat one column off the body.
The balloon also dropped the trailing space the original emits.
So the snapshots this repository checked in were of visibly broken art.
And it weighed 772 kB for a program that prints a cow, which the backends then multiply.
The Bash conversion was 7.73 MB.
That is too large to open and read as the "look at the generated source" example it exists to be.

## Decision

An app's upstream can be *wrong for the example* while the app is small enough to reimplement.
Then write and publish the implementation.
Do not adopt a better-behaved third party, and do not commit sources here.
The criterion is *upstream stays upstream* (decision 9).
This repository holds a URL and a hash.
A separate repository owns the implementation, its tests, and its releases.
Our own upstream is still an upstream.
So nothing about the fetch, the fixed version, or the audit changes.

The published implementation carries its own correctness argument.
That repository keeps a copy of the original `cowsay` 3.03.
A differential test suite runs the implementation against it under the host Perl.
The suite compares standard output, standard error and the exit code over 738 cases.
So "byte-identical to the original" is checked there and not assumed here.

## Rejected alternatives

- **Keep the registry build.**
  The README and every backend's snapshot would keep showing the broken art.
  The size also defeats the example.
- **Commit the C sources here.**
  Decision 9 keeps app artifacts out of this repository.
  This `cowsay` implementation has its own Perl-differential suite.
  Such a project has no business inside a wasm converter's tree.
- **Build it from source in `setup.sh`, as `sqlite3` and `minigzip` are (decision 92).**
  Those are third-party sources we must build, because nobody publishes a WASI binary.
  For our own code we control the release.
  So a fixed release asset costs developers here nothing, and keeps the bytes identical everywhere.
- **Adopt another existing `cowsay` port.**
  Every one surveyed either draws the same broken art or carries a language runtime of its own.
  That is the size problem again.

## Consequences

- Positive: the example shows the cow the original draws.
  The Bash conversion drops from 7.73 MB to 1.13 MB, small enough to read.
  The module audits as pure baseline (`agents/apps-audit.md`).
  That is because its release runs the same `wasm-opt` pass the locally-built modules get.
- Positive: the app now exercises the WASI file system surface in the default `cargo test` run.
  That is because the lookup of cow files honours `COWPATH`.
  Its imports go from 8 functions to 17.
  The 8 cover standard streams, arguments, environment, `random_get` and `proc_exit`.
  The added ones are `path_open`, `fd_readdir`, `path_filestat_get` and the preopen calls.
- Negative: `app/cowsay` rows are not comparable across this change.
  A speed or size record from before it cannot be compared row by row against a later one.
  That is because the module is a different program.
  The records are dated snapshots, so the break is visible rather than silent.
- Carry-over: some cases follow the fixed release:
  - the `cowsay` snapshots under `examples/apps/snapshots/`;
  - the two interpreter cases that run `cowsay` as an inner guest (`TOYWASM_COWSAY`, `WASM3_COWSAY`).

  Moving to a new release means regenerating them with `cargo xtask update-snapshots cowsay`.

See also:
- [decision 9](9-example-apps-from-registry.md) (the policy of fetching at a fixed version, which this stays inside);
- [decision 39](39-running-wasm-opt.md) (the `wasm-opt` pass the release mirrors);
- [decision 92](92-wasi-sdk-c-toolchain.md) (the C toolchain both use).

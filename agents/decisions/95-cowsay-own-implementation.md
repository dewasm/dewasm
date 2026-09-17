# Decision 95: cowsay Comes From Our Own Implementation, Published Upstream

Status: **Accepted, 2026-09-18.**
Implemented: [`examples/apps/scripts/cowsay.sh`](../../examples/apps/scripts/cowsay.sh) pins [dewasm/cowsay.wasm](https://github.com/dewasm/cowsay.wasm) v0.1.0, a C reimplementation of cowsay 3.03 whose output is byte-identical to the original Perl script.
The fetch-and-pin mechanism of decision 9 is unchanged; only the upstream this app points at is.

## Context

The showpiece app was the Wasmer registry's cowsay 0.3.0, a Rust clone.
It made a poor showpiece on both counts a showpiece is judged by.
Its cow was drawn wrong: the legs line sat one column off the body, and the balloon dropped the trailing space the original emits, so the snapshots this repository checked in were of visibly broken art.
And it weighed 772 kB for a program that prints a cow, which the backends then multiply: the Bash conversion was 7.73 MB, too large to open and read as the "look at the generated source" demo it exists to be.

## Decision

When an app's upstream is *wrong for the demo* and the app is small enough to reimplement, write and publish the implementation rather than pinning a better-behaved third party or committing sources here.
The criterion is *upstream stays upstream* (decision 9): this repository holds a URL and a hash, and a separate repository owns the implementation, its tests, and its releases.
Our own upstream is still an upstream, so nothing about the fetch, the pin, or the audit changes.

The published implementation carries its own correctness argument: a differential test suite runs it against the vendored cowsay 3.03 under the host Perl and compares stdout, stderr and the exit code over 738 cases, so "byte-identical to the original" is enforced there and not assumed here.

## Rejected alternatives

- **Keep the registry build**: the broken art is what the README and every backend's snapshot would keep showing, and the size defeats the demo.
- **Commit the C sources here**: decision 9 keeps app artifacts out of this repository, and a cowsay implementation with its own Perl-differential suite has no business inside a wasm converter's tree.
- **Build it from source in `setup.sh`**, as sqlite3 and minigzip are (decision 92): those are third-party sources we must build because nobody publishes a WASI binary; for our own code we control the release, so a pinned release asset costs contributors nothing and keeps the bytes identical everywhere.
- **Pin another existing cowsay port**: every one surveyed either draws the same broken art or carries a language runtime of its own, which is the size problem again.

## Consequences

- Positive: the demo shows the cow the original draws, and the Bash conversion drops from 7.73 MB to 1.13 MB, small enough to read.
  The module audits as pure baseline (`agents/apps-audit.md`), since its release runs the same `wasm-opt` pass the locally-built modules get.
- Positive: the app now exercises the WASI filesystem surface in the fast lane, because cowfile lookup honours `COWPATH`.
  Its imports go from 8 functions (stdio, args, environ, `random_get`, `proc_exit`) to 17, adding `path_open`, `fd_readdir`, `path_filestat_get` and the preopen calls.
- Negative: a speed or size record taken before this change cannot be compared row by row against a later one for `app/cowsay`, since the module is a different program.
  The records are dated snapshots, so the break is visible rather than silent.
- Carry-over: the cowsay snapshots under `examples/apps/snapshots/` and the two interpreter cases that run cowsay as an inner guest (`TOYWASM_COWSAY`, `WASM3_COWSAY`) follow the pinned release; bumping the pin means regenerating them with `cargo xtask update-snapshots cowsay`.

See also: [decision 9](9-example-apps-from-registry.md) (the fetch-and-pin policy this stays inside), [decision 39](39-wasm-opt-preprocessing.md) (the wasm-opt pass the release mirrors), [decision 92](92-wasi-sdk-c-toolchain.md) (the C toolchain both use).

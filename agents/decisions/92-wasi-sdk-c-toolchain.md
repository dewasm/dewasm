# Decision 92: `wasi-sdk` as the C Toolchain for the Locally-Built Modules

Status: **Accepted, 2026-09-12.**
Implemented in these places (issue #306):
- `examples/apps/scripts/common.sh` (`require_wasi_sdk`, `wasi_sdk_clang`, `wasi_sdk_stamp`);
- every C build under `examples/apps/scripts/`;
- `benchmarks/c/build.sh`;
- the CI apps-cache job (`.github/workflows/ci.yml`);
- `docs/testing.md`;
- the compatibility sources `examples/apps/src/pcap_wasi/`;
- the compatibility source `examples/apps/src/sqlite3_shell_wasi_compat.c`.

Every snapshot passed unchanged against the rebuilt artifacts.
[Decision 101](101-tool-versions-in-mise.md) replaced the install policy.
`mise.toml` now states the SDK version and sets `WASI_SDK_PATH`, for local development and for CI.
The choice of `wasi-sdk`, the variable, `--no-wasm-opt`, and the stamp all stand.

## Context

[Decision 22](22-sqlite3-built-from-source.md) chose `zig cc` over `wasi-sdk` for the from-source builds.
The reason was that Zig is one binary from Homebrew, with the WASI headers and libraries built in.
The project uses Zig purely as a packaging of Clang + `wasi-libc`, and that packaging caused problems:

- The `mruby` exception-handling build ([decision 69](69-exception-handling-accepted-input.md)) hit a Zig 0.16 trap.
  Linking with `-mexception-handling` makes Zig rebuild the `setjmp` runtime of `wasi-libc`.
  The rebuild leaves out the SJLJ flags and fails.
  The local fix compiled `rt.c` from the files Zig ships, at a path parsed from `zig env`.
  Any Zig layout change breaks that, and Zig makes breaking changes on every 0.x release.
- Homebrew updates the local Zig implicitly.
  Meanwhile CI installs a fixed version, with a "keep in sync" comment that nothing checks.
- `zig cc` silently ignores `-nostartfiles` for wasm, worked around in `benchmarks/c/build.sh`.

`wasi-sdk` supports the `setjmp`/`longjmp` lowering officially.
Its shipped `libsetjmp` takes `-mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false`.
Together they replace the `rt.c` local fix entirely.
The SDK ships Clang and `wasi-libc` at fixed versions.
They come as one tarball per platform, verified by checksum.
CI already holds Binaryen and `wasi-vfs` at fixed versions in the same way.

## Decision

**The C toolchain is chosen for the stability and controllability of its Clang + `wasi-libc` packaging.**
**It is not chosen for ease of installation.**
**A packaging may alter Clang behavior outside our control.**
**Such a packaging loses to one at a version we choose explicitly.**
Concretely:

- All locally-compiled modules build with `wasi-sdk` through `wasi_sdk_clang`.
  That function is in `examples/apps/scripts/common.sh`.
  The `WASI_SDK_PATH` environment variable locates the SDK.
  A missing SDK fails loudly, naming the fixed version and the URL to fetch it from ([decision 15](15-tests-fail-not-skip.md)).
  How a developer installs it is deliberately their choice.
  The repository names a fixed version only where it installs the SDK itself (CI).
  It also documents the version (`docs/testing.md`).
- Every build passes `--no-wasm-opt`.
  Otherwise the `wasi-sdk` Clang driver runs a `wasm-opt` found on `PATH` over the linked module.
  That uncontrolled pass stripped the name section the sqlite3-mod check needs.
  It also re-inlined the module's split functions.
  The only `wasm-opt` pass is `wasm_opt_inplace` ([decision 39](39-running-wasm-opt.md)).
- Each stamp key folds in `wasi_sdk_stamp` (`cc:wasi-sdk`), the toolchain's identity and not its version.
  That is because artifact bytes may vary across toolchain versions.
  By decision 22's rule, behavior is what the snapshots keep fixed.
  An honest version stamp would have to read the installed SDK.
  But `setup.sh --check` must work without the installed SDK.
- The header and symbol surface of `wasi-libc` is narrower than the full `musl` surface Zig exposed.
  First-party compatibility files cover the gap.
  They fail at runtime the way a sandboxed module does.
  `examples/apps/src/pcap_wasi/` covers `<netdb.h>`, `<net/if.h>`, and `socket()`.
  `examples/apps/src/sqlite3_shell_wasi_compat.c` covers `getpid`.
  The gap is never covered by patching the upstream sources, which stay at fixed versions.

## Rejected alternatives

- **Stay on Zig.**
  The 0.16 trap is one instance of a class of problems that repeats.
  The local fix depended on Zig internals.
  The Zig from Homebrew drifts from the version CI installs, without anyone choosing an update.
  Decision 22 was decided by ease of installation.
  That is worth less than a repeated maintenance cost on the EH fixture.
  The EH fixture is the one app whose build is hardest to debug.
- **Have `setup.sh` fetch and install the SDK itself.**
  It would keep the version in one place, but it dictates how every developer manages toolchains.
  The environment-variable contract keeps the repository's requirement minimal.
  This can be revisited if new developers find the step hard in practice.
- **Fold the SDK version into the stamps.**
  That is honest only if read from the installed SDK.
  Then `--check` would require a toolchain the CI test jobs deliberately do not install.
- **Patch the app sources for the missing headers.**
  Upstream sources are patched only for behavior we own (the sqlite3 VDBE split).
  A missing platform surface belongs in our own compatibility files beside the binding sources.

## Consequences

- Positive: the `mruby` build depends on no toolchain internals.
  Clang and `wasi-libc` versions are chosen together and deliberately.
  The hidden driver `wasm-opt` is turned off everywhere.
  So decision 39's constraints hold by construction.
- Negative: installing the SDK is a manual step, where Zig was one Homebrew command.
  The step is a tarball plus `WASI_SDK_PATH`.
  A future C app may need compatibility additions where `wasi-libc` leaves out headers `musl` carries.
- Carry-over: no tool checks that the local and CI SDK versions match.
  Drift surfaces only as behavior differences, which the snapshot suites detect.
  The Lua audit entry (`agents/apps-audit.md`) records a linker crash measured under Zig's Clang 21.
  A retry now goes through the `libsetjmp` that `wasi-sdk` ships.

See also:
- [decision 9](9-example-apps-from-registry.md) (the policy on fixed versions);
- [decision 22](22-sqlite3-built-from-source.md) (the choice this reverses in part);
- [decision 39](39-running-wasm-opt.md) (the `wasm-opt` pass kept singular);
- [decision 69](69-exception-handling-accepted-input.md) (the EH fixture that forced the issue).

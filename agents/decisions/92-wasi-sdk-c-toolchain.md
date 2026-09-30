# Decision 92: wasi-sdk as the C Toolchain for the Locally-Built Modules

Status: **Accepted, 2026-09-12.**
Implemented in these places (issue #306):
- `examples/apps/scripts/common.sh` (`require_wasi_sdk`, `wasi_sdk_clang`, `wasi_sdk_stamp`);
- every C build under `examples/apps/scripts/`;
- `benchmarks/c/build.sh`;
- the CI apps-cache job (`.github/workflows/ci.yml`);
- `docs/testing.md`;
- the compat sources `examples/apps/src/pcap_wasi/` and `examples/apps/src/sqlite3_shell_wasi_compat.c`.

Every snapshot passed unchanged against the rebuilt artifacts.

## Context

[Decision 22](22-sqlite3-built-from-source.md) chose `zig cc` over wasi-sdk for the from-source builds.
The reason was that zig is one brew-installable binary with the wasi sysroot built in.
The project uses zig purely as a packaging of clang + wasi-libc, and that packaging leaked:

- The mruby exception-handling build ([decision 69](69-exception-handling-accepted-input.md)) hit a zig 0.16 trap.
  Linking with `-mexception-handling` makes zig rebuild wasi-libc's setjmp runtime.
  The rebuild omits the SJLJ flags and fails.
  The workaround compiled rt.c out of zig's own sysroot, at a path parsed from `zig env`.
  Any zig layout change breaks that, and zig makes breaking changes on every 0.x release.
- Homebrew upgrades the local zig implicitly.
  Meanwhile CI pins a version with a "keep in sync" comment that nothing enforces.
- `zig cc` silently ignores `-nostartfiles` for wasm, worked around in `benchmarks/c/build.sh`.

wasi-sdk supports the setjmp/longjmp lowering officially.
Its shipped `libsetjmp` takes `-mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false`.
Together they replace the rt.c workaround entirely.
The SDK pins clang and wasi-libc together as a checksum-verified per-platform tarball.
That is the same pinning shape CI already uses for binaryen and wasi-vfs.

## Decision

**The C toolchain is chosen for the stability and controllability of its clang + wasi-libc packaging.**
**It is not chosen for installation convenience.**
**A packaging may alter clang behavior outside our control.**
**Such a packaging loses to one whose version we pin explicitly.**
Concretely:

- All locally-compiled modules build with wasi-sdk through `wasi_sdk_clang`.
  That function is in `examples/apps/scripts/common.sh`.
  The `WASI_SDK_PATH` environment variable locates the SDK.
  A missing SDK fails loudly, naming the pinned version and download URL ([decision 15](15-tests-fail-not-skip.md)).
  How a developer installs it is deliberately their choice.
  The repository pins the version only where it installs the SDK itself (CI).
  It also documents the version (`docs/testing.md`).
- Every build passes `--no-wasm-opt`.
  Otherwise the wasi-sdk clang driver runs a wasm-opt found on PATH over the linked module.
  That uncontrolled pass stripped the name section the sqlite3-mod check needs.
  It also re-inlined the module's split functions.
  The only wasm-opt pass is `wasm_opt_inplace` ([decision 39](39-wasm-opt-preprocessing.md)).
- Each stamp key folds in `wasi_sdk_stamp` (`cc:wasi-sdk`), the toolchain's identity and not its version.
  Artifact bytes may vary across toolchain versions.
  By decision 22's rule, behavior is what the snapshots pin.
  An honest version stamp would have to read the installed SDK.
  But `setup.sh --check` must work without the installed SDK.
- wasi-libc's header and symbol surface is narrower than the full musl surface zig exposed.
  First-party compat files cover the gap, and fail at runtime the way a sandboxed module does.
  `examples/apps/src/pcap_wasi/` covers `<netdb.h>`, `<net/if.h>`, and `socket()`.
  `examples/apps/src/sqlite3_shell_wasi_compat.c` covers `getpid`.
  The gap is never covered by patching the pinned upstream sources.

## Rejected alternatives

- **Stay on zig.**
  The 0.16 trap is one instance of a recurring class.
  The workaround depended on zig internals.
  The brew-installed zig drifts from CI's pin without anyone choosing an upgrade.
  Decision 22 was decided by install convenience.
  That does not outweigh a recurring maintenance tax on the EH fixture.
  The EH fixture is the one app whose build is hardest to debug.
- **Have `setup.sh` download and install the SDK itself.**
  It would centralize the pin, but it dictates how every developer manages toolchains.
  The environment-variable contract keeps the repository's requirement minimal.
  This can be revisited if onboarding friction proves real.
- **Fold the SDK version into the stamps.**
  That is honest only if read from the installed SDK.
  Then `--check` would require a toolchain the CI test jobs deliberately do not install.
- **Patch the app sources for the missing headers.**
  Upstream sources are patched only for behavior we own (the sqlite3 VDBE split).
  A missing platform surface belongs in our own compat files beside the binding sources.

## Consequences

- Positive: the mruby build depends on no toolchain internals.
  Clang and wasi-libc versions are chosen together and deliberately.
  The hidden driver wasm-opt is disabled everywhere.
  So decision 39's constraints hold by construction.
- Negative: installing the SDK is a manual step, where zig was one brew command.
  The step is a tarball plus `WASI_SDK_PATH`.
  A future C app may need compat additions where wasi-libc omits headers musl carries.
- Carry-over: local and CI SDK versions are not machine-enforced.
  Drift surfaces only as behavior differences, which the snapshot suites detect.
  The Lua audit entry (`agents/apps-audit.md`) records a linker crash measured under zig's clang 21.
  A retry now goes through wasi-sdk's shipped libsetjmp.

See also:
- [decision 9](9-example-apps-from-registry.md) (the pin policy);
- [decision 22](22-sqlite3-built-from-source.md) (the choice this reverses in part);
- [decision 39](39-wasm-opt-preprocessing.md) (the wasm-opt pass kept singular);
- [decision 69](69-exception-handling-accepted-input.md) (the EH fixture that forced the issue).

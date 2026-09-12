# Decision 92: wasi-sdk as the C Toolchain for the Locally-Built Modules

Status: **Accepted, 2026-09-12.**
Implemented in `examples/apps/scripts/common.sh` (`require_wasi_sdk`, `wasi_sdk_clang`, `wasi_sdk_stamp`), every C build under `examples/apps/scripts/`, `benchmarks/c/build.sh`, the CI apps-cache job (`.github/workflows/ci.yml`), and `docs/testing.md`, together with the compat sources `examples/apps/src/pcap_wasi/` and `examples/apps/src/sqlite3_shell_wasi_compat.c` (issue #306).
Every snapshot passed unchanged against the rebuilt artifacts.

## Context

[Decision 22](22-sqlite3-built-from-source.md) chose `zig cc` over wasi-sdk for the from-source builds because zig is one brew-installable binary with the wasi sysroot built in.
The project uses zig purely as a packaging of clang + wasi-libc, and that packaging leaked:

- The mruby exception-handling build ([decision 69](69-exception-handling-accepted-input.md)) hit a zig 0.16 trap: linking with `-mexception-handling` makes zig rebuild wasi-libc's setjmp runtime without the SJLJ flags and fail.
  The workaround compiled rt.c out of zig's own sysroot at a path parsed from `zig env`, which any zig layout change breaks, and zig makes breaking changes on every 0.x release.
- Homebrew upgrades the local zig implicitly while CI pins a version with a "keep in sync" comment that nothing enforces.
- `zig cc` silently ignores `-nostartfiles` for wasm, worked around in `benchmarks/c/build.sh`.

wasi-sdk supports the setjmp/longjmp lowering officially: its shipped `libsetjmp` with `-mllvm -wasm-enable-sjlj -mllvm -wasm-use-legacy-eh=false` replaces the rt.c workaround entirely, and the SDK pins clang and wasi-libc together as a checksum-verified per-platform tarball, the same pinning shape CI already uses for binaryen and wasi-vfs.

## Decision

**The C toolchain is chosen for the stability and controllability of its clang + wasi-libc packaging, not for installation convenience: a packaging that alters clang behavior outside our control loses to one whose version we pin explicitly.**
Concretely:

- All locally-compiled modules build with wasi-sdk through `wasi_sdk_clang` in `examples/apps/scripts/common.sh`, located by the `WASI_SDK_PATH` environment variable.
  A missing SDK fails loudly naming the pinned version and download URL ([decision 15](15-tests-fail-not-skip.md)); how a developer installs it is deliberately their choice, and the repository pins the version only where it installs the SDK itself (CI) and documents it (`docs/testing.md`).
- Every build passes `--no-wasm-opt`: the wasi-sdk clang driver otherwise runs a wasm-opt found on PATH over the linked module, an uncontrolled pass that stripped the name section the sqlite3-mod check needs and re-inlined its split functions.
  The only wasm-opt pass is `wasm_opt_inplace` ([decision 39](39-wasm-opt-preprocessing.md)).
- Each stamp key folds in `wasi_sdk_stamp` (`cc:wasi-sdk`), the toolchain's identity and not its version: artifact bytes may vary across toolchain versions (decision 22's rule, behavior is what the snapshots pin), and an honest version stamp would have to read the installed SDK, which `setup.sh --check` must work without.
- wasi-libc's header and symbol surface is narrower than the full musl surface zig exposed; the gap is covered by first-party compat files that fail at runtime the way a sandboxed module does (`examples/apps/src/pcap_wasi/` for `<netdb.h>`, `<net/if.h>`, and `socket()`; `examples/apps/src/sqlite3_shell_wasi_compat.c` for `getpid`), never by patching the pinned upstream sources.

## Rejected alternatives

- **Stay on zig.**
  The 0.16 trap is one instance of a recurring class: the workaround depended on zig internals, and the brew-installed zig drifts from CI's pin without anyone choosing an upgrade.
  The install convenience that decided decision 22 does not outweigh a recurring maintenance tax on the EH fixture, the one app whose build is hardest to debug.
- **Have `setup.sh` download and install the SDK itself.**
  It would centralize the pin, but it dictates how every developer manages toolchains; the environment-variable contract keeps the repository's requirement minimal, and this can be revisited if onboarding friction proves real.
- **Fold the SDK version into the stamps.**
  Honest only if read from the installed SDK, which would make `--check` require a toolchain the CI test jobs deliberately do not install.
- **Patch the app sources for the missing headers.**
  Upstream sources are patched only for behavior we own (the sqlite3 VDBE split); a missing platform surface belongs in our own compat files beside the binding sources.

## Consequences

- Positive: the mruby build depends on no toolchain internals; clang and wasi-libc versions are chosen together and deliberately; the hidden driver wasm-opt is disabled everywhere, so decision 39's constraints hold by construction.
- Negative: installing the SDK is a manual step (tarball plus `WASI_SDK_PATH`) where zig was one brew command; a future C app may need compat additions where wasi-libc omits headers musl carries.
- Carry-over: local and CI SDK versions are not machine-enforced; drift surfaces only as behavior differences, which the snapshot suites detect.
  The Lua audit entry (`agents/apps-audit.md`) records a linker crash measured under zig's clang 21; a retry now goes through wasi-sdk's shipped libsetjmp.

See also: [decision 9](9-example-apps-from-registry.md) (the pin policy), [decision 22](22-sqlite3-built-from-source.md) (the choice this reverses in part), [decision 39](39-wasm-opt-preprocessing.md) (the wasm-opt pass kept singular), [decision 69](69-exception-handling-accepted-input.md) (the EH fixture that forced the issue).

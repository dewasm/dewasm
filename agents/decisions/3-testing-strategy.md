# Decision 3: Testing Strategy (Specification Testsuite on Real Interpreters)

Status: **Accepted, 2026-07-23.**
Recorded afterwards; implemented in `crates/dewasm-test-helper/src/spec.rs` for the Ruby backend.
The skip policy (selected file list, plain skip counts) was revised the same day by [decision 8](8-latest-testsuite-support-matrix.md).
The harness now runs every testsuite file.
It requires each skip to be attributable to a declared-unsupported feature.
Differential testing of WASI programs against Wasmtime is done manually so far.
Running it automatically remains open.

## Context

`dewasmify`'s whole value is exact semantics across six target languages.
That cannot be maintained by hand-picked unit tests.
It needs the official WebAssembly specification testsuite, applied uniformly to every backend.
The suite must be executed the way users will actually run the output.

## Decision

- **The official `WebAssembly/testsuite` is a Git submodule at `tests/spec`**, shallow, fixed at one commit.
  So upstream changes never break CI silently, and the tested revision is part of history.
- **The harness converts `.wast` files into assertion scripts in the target language.**
  **It runs them on the real interpreter** (`ruby`, later `bash`, `java`, ...).
  Criterion: *what gets tested must be the shipped artifact*.
  That is generated source + embedded runtime on a stock interpreter.
  It is not an in-process simulation of the artifact.
- **Definition of done for a backend = this harness passes.**
  Adding a language backend means making the shared harness pass for it.
  There is no backend-private notion of "works".
- **Directives that exercise unsupported features are counted as `skip`.**
  The count is driven by conversion failure of the module they target.
  The converter's clear-error contract from decision 0 doubles as the skip signal.
  `assert_invalid` / `assert_malformed` are checked on the Rust side: conversion must fail.
- **Deviations live in an `EXPECTED_FAILURES` list** in the harness.
  Each entry carries a count and a reason comment.
  The file still runs, so regressions in its passing assertions are caught.
  A list entry is a debt marker; it does not excuse the failure.
  Fixing the cause is the default; adding an entry needs the reason written down.

## Rejected alternatives

- **A reference interpreter inside `dewasmify`**: repeats Wasmtime and the reference interpreter.
  It also tests the wrong thing (our interpreter, not our generated code).
- **Differential testing only** (run wasm under Wasmtime vs. converted output).
  It is good for WASI-level end-to-end checks and kept as a complement.
  But it cannot locate a fault in per-instruction semantics the way ~20k targeted assertions do.

## Consequences

- Positive: backend bugs surface as named `.wast` lines.
  The Ruby backend's NaN and rounding defects (decision 2) were all found this way.
- Negative: harness runtime scales with interpreter speed.
  That is fine for Ruby (~5 s) and a real concern for Bash.
  Bash will need a selected subset in CI with full runs out-of-band (accepted in advance).
- The upstream testsuite tracks the latest specification.
  So newly added proposal files skip until the corresponding feature lands.

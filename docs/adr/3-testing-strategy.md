# ADR-3 — Testing Strategy: Spec Testsuite on Real Interpreters

Status: **Accepted, 2026-07-23.** Backfilled; implemented in `crates/dewasm-test-helper/src/spec.rs` for the Ruby backend. The skip policy (curated file list, bare skip counts) was revised the same day by [ADR-8](8-latest-testsuite-support-matrix.md): the harness now runs every testsuite file and requires each skip to be attributable to a declared- unsupported feature. Differential testing of WASI programs against wasmtime is done manually so far; automating it remains open.

## Context

dewasmify's whole value is semantic fidelity across six target languages. That cannot be maintained by hand-picked unit tests; it needs the official WebAssembly spec testsuite, applied uniformly to every backend, executed the way users will actually run the output.

## Decision

- **The official `WebAssembly/testsuite` is a git submodule at `tests/spec`**, shallow, pinned to a commit, so upstream churn never breaks CI silently and the tested revision is part of history.
- **The harness converts `.wast` files into assertion scripts in the target language and runs them on the real interpreter** (`ruby`, later `bash`, `java`, ...). Criterion: *what gets tested must be the shipped artifact* — generated source + embedded runtime on a stock interpreter, not an in-process simulation of it.
- **Definition of done for a backend = this harness passes.** Adding a language backend means making the shared harness green for it; no backend-private notion of "works".
- **Directives that exercise unsupported features are counted as `skip`**, driven by conversion failure of the module they target (the converter's clear-error contract from ADR-0 doubles as the skip signal). `assert_invalid` / `assert_malformed` are checked on the Rust side: conversion must fail.
- **Deviations live in an `EXPECTED_FAILURES` ledger** in the harness, each entry carrying a count and a reason comment. The file still runs, so regressions in its passing assertions are caught. A ledger entry is a debt marker, not an exemption: fixing the cause is the default, adding an entry needs the reason written down.

## Rejected alternatives

- **A reference interpreter inside dewasmify** — duplicates wasmtime/the spec interpreter, and tests the wrong thing (our interpreter, not our generated code).
- **Differential testing only** (run wasm under wasmtime vs. converted output) — good for WASI-level end-to-end checks and kept as a complement, but it cannot pinpoint per-instruction semantics the way ~20k targeted assertions do.

## Consequences

- Positive: backend bugs surface as named `.wast` lines; the Ruby backend's NaN and rounding defects (ADR-2) were all found this way.
- Negative: harness runtime scales with interpreter speed — fine for Ruby (~5 s), a real concern for Bash, which will need a curated subset in CI with full runs out-of-band (accepted in advance).
- The upstream testsuite tracks the latest spec, so newly added proposal files simply skip until the corresponding feature lands.

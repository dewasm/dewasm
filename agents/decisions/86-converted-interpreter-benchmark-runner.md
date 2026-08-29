# Decision 86: wasm3 as the Converted-Interpreter Benchmark Runner

Status: **Accepted, 2026-08-29.** The speed suite carries four `wasm3-*` runners (`Kind::ConvertedInterpreter` in [`crates/xtask/src/bench/runner.rs`](../../crates/xtask/src/bench/runner.rs)): the meta-WASI wasm3 v0.5.0 build from the app cache, converted standalone by the Ruby and Python backends, interpreting each workload on ruby, ruby+yjit, cpython, and pypy.

## Context

The suite compares dewasm's converted output against the wasm interpreters hand-written in the target languages (pywasm, wardite).
That comparison mixes categories: dewasm's output is converted ahead of time, while those interpreters load an arbitrary module at run time.
A wasm interpreter that is itself dewasm output closes the category gap: it also loads an arbitrary module at run time, and everything below that module is dewasm's own code, so the pairing measures dewasm against a hand-written interpreter on equal terms.

## Decision

The interpreter the suite converts is wasm3 v0.5.0, and the criterion is the interpreter's own native speed: conversion multiplies the interpreter's cost by a roughly constant factor (about 270x on ruby+yjit for interpreter-shaped code, measured 2026-08-29), so only an interpreter that is fast natively keeps the converted stack ahead of the hand-written interpreters.
wasm3 interprets at roughly 6x wasmtime natively and the converted build won every same-language pairing against wardite and pywasm (1.3x to 3.4x on `wat/i32_alu` and `wat/mem_rw`, 2026-08-29); toywasm sits near 300x natively and the converted build lost the same pairings by 5x to 14x, measured the same day with the same harness discipline.

## Rejected alternatives

- **Converted toywasm as the runner.**
  Loses every same-language pairing by 5x to 14x (measured above): its own interpretation overhead, not the conversion, is what sinks it.
  It stays an e2e app; the two interpreters' cases are deliberately parallel (`toywasm_cowsay`, `wasm3_cowsay`).
- **Self-application: dewasm converted by itself, converting the workload at load time and evaluating the result.**
  Executes at direct-conversion speed but pays roughly 250x to 300x the native conversion time at every cold load (measured 2026-08-02); a different trade, set aside rather than folded into the benchmark matrix.
- **A committed driver script per host, the pywasm/wardite shape.**
  Unnecessary: the standalone interface's `--dir` shim already carries the workload's directory and module path through to the guest, and the runner reuses the same `Workshop` conversion cache as the `dewasm-*` runners, so there is no separate artifact to provision.

## Consequences

- Positive: the same-category comparison ("a runtime-loading wasm runtime in pure Ruby/Python") is measured continuously instead of living in a one-off experiment.
- Negative: wasm3's dispatch nests one host-language call per guest opcode until a loop or return unwinds it, so the ruby runners carry `RUBY_THREAD_VM_STACK_SIZE` in their launch environment, and workloads that still exceed a host's limits get measured exclusions in [`crates/xtask/src/bench/workload.rs`](../../crates/xtask/src/bench/workload.rs).
- Carry-over: `wasm3-*` labels classify as the interpreter family in the charts; the native `wasm3` runner stays in the native family, so the two readings of "wasm3" never share a color.

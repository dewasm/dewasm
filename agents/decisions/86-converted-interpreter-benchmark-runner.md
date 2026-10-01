# Decision 86: wasm3 as the Converted-Interpreter Benchmark Runner

Status: **Accepted, 2026-08-29.**
The speed suite carries four `wasm3-*` runners.
Their kind is `Kind::ConvertedInterpreter` in [`crates/xtask/src/bench/runner.rs`](../../crates/xtask/src/bench/runner.rs).
They take the MetaWASI wasm3 build from the app cache (fixed at v0.9.0).
The Ruby and Python backends convert it standalone.
The converted builds interpret each workload on Ruby, Ruby with YJIT, CPython, and PyPy.

## Context

The suite compares dewasm's converted output against wasm interpreters.
Those interpreters, `pywasm` and `wardite`, are hand-written in the target languages.
That comparison mixes categories.
That is because dewasm's output is converted ahead of time.
Those interpreters load an arbitrary module at run time.
A wasm interpreter that is itself dewasm output closes the category gap.
That is because it also loads an arbitrary module at run time.
Everything below that module is dewasm's own code.
So the pairing measures dewasm against a hand-written interpreter on equal terms.

## Decision

The interpreter the suite converts is wasm3 (fixed at v0.9.0).
The comparative measurements below were taken on the v0.5.0 build.
The v0.9.0 build measured the same speed on the same day.

The criterion is the interpreter's own native speed.
That is because conversion multiplies the interpreter's cost by a roughly constant factor.
That factor is about 270x on Ruby with YJIT for interpreter-shaped code, measured 2026-08-29.
So the interpreter must be fast natively.
Only then does the converted stack stay ahead of the hand-written interpreters.

| Interpreter | Native speed | Converted build against `wardite` and `pywasm` |
| --- | --- | --- |
| wasm3 | roughly 6x Wasmtime natively | won every same-language pairing, by 1.3x to 3.4x on `wat/i32_alu` and `wat/mem_rw` |
| `toywasm` | near 300x natively | lost the same pairings by 5x to 14x |

Both rows were measured on 2026-08-29 with the same harness discipline.

## Rejected alternatives

- **Converted `toywasm` as the runner.**
  It loses every same-language pairing by 5x to 14x (measured above).
  Its own interpretation overhead, not the conversion, is what sinks it.
  It stays an e2e app.
  The two interpreters' cases are deliberately parallel (`toywasm_cowsay`, `wasm3_cowsay`).
- **Self-application: dewasm converted by itself.**
  The converted dewasm converts the workload at load time and evaluates the result.
  It executes at direct-conversion speed.
  But every cold load pays roughly 250x to 300x the native conversion time (measured 2026-08-02).
  That is a different trade, set aside rather than folded into the benchmark matrix.
- **A committed driver script per host, the `pywasm`/`wardite` shape.**
  It is unnecessary.
  The standalone interface's `--dir` shim already carries the workload's directory to the guest.
  It carries the module path as well.
  The runner also reuses the same `Workshop` conversion cache as the `dewasm-*` runners.
  So there is no separate artifact to provision.

## Consequences

- Positive: the same-category comparison is measured continuously.
  It no longer lives in a one-off experiment.
  The comparison is "a runtime-loading wasm runtime in pure Ruby/Python".
- Negative: wasm3's dispatch nests one host-language call per guest instruction.
  The nesting lasts until a loop or return unwinds it.
  So the Ruby runners carry `RUBY_THREAD_VM_STACK_SIZE` in their launch environment.
  Workloads that still exceed a host's limits get measured exclusions.
  Those are in [`crates/xtask/src/bench/workload.rs`](../../crates/xtask/src/bench/workload.rs).
- Carry-over: `wasm3-*` labels classify as the interpreter family in the charts.
  The native `wasm3` runner stays in the native family.
  So the two readings of "wasm3" never share a color.

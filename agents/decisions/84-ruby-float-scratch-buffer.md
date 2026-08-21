# Decision 84: Per-Receiver `IO::Buffer` Scratch for the Ruby Float Bit Conversions

Status: **Accepted, 2026-08-21.**
`runtime/ruby/units/rt/scratch.rb` holds the buffer and the five conversions (`f32`, `f32_bits`, `f32_from_bits`, `f64_bits`, `f64_from_bits`) borrow it; `Rt::Memory` includes `Rt` so its f32 accessors reach them on the memory instance.

## Context

The conversions reinterpreted a value by writing it into a packed String and reading it back (`[x].pack("E").unpack1("Q<")`), which allocates an Array and a String per call.
On a Rust HTML-to-PDF renderer whose layout and shaping stages keep their data in f32, those five lines produced 1.09M of the 1.10M String allocations per render, around 30% of all allocations and around 4% of wall time (issue #261).

`IO::Buffer#set_value`/`#get_value` reinterprets with no allocation, and the artifacts already require Ruby 3.4+ for their `IO::Buffer` linear memory ([decision 33](33-ruby-io-buffer-memory.md)).
The buffer has to live somewhere, though, and a store followed by its read-back is two operations: whoever else can reach that buffer in between gets the other party's value back.
The `pack` version had no such state, so the placement decides whether the change costs correctness.

## Decision

**Runtime state introduced for speed is placed at the coarsest granularity that already cannot be shared between threads.**
For this runtime that granularity is the artifact instance: instances hold their own linear memory, so two of them on two threads is the supported shape ([decision 45](45-rails-sqlite3-shim-example.md)'s connection pool), while two threads inside one instance is not.

So the scratch is `@scratch` on the receiver, created by `rt/scratch` and read inline (`s = @scratch || scratch`) at each use.
Generated bodies call the runtime as included instance methods, where the receiver is the artifact instance.
The memory units called it as `Rt.f32_bits`, whose receiver is one module per class, so `memory/_class` now includes `Rt` and their f32 accessors call the conversions bare, on the memory instance.

Measured on a converted loop of 3M iterations, nine conversions each (macOS arm64, ruby 4.0.4, best of six):

| scratch placement | wall | allocations | corrupted runs |
| --- | --- | --- | --- |
| `pack`/`unpack1` (before) | 5075 ms | 87.0M | 0 / 480 |
| per receiver, `@scratch` read inline | 3790 ms | 6.0M | 0 / 480 |
| module-level constant | 3443 ms | 6.0M | 29 / 480 |
| per receiver, through a method call | 4435 ms | 6.0M | 0 / 480 |
| `Thread.current[:...]` | 4898 ms | 6.0M | 0 / 480 |

The corruption column runs the same loop 120 times on each of four threads, each thread with its own artifact instance, and compares against a single-threaded reference result.

## Rejected alternatives

- **A module-level constant buffer.**
  9% faster than the chosen placement and the natural reading of "one reusable scratch", but every instance of the class shares it: four threads on four instances returned a wrong float in 29 of 480 runs, where the `pack` version returns none.
  A silent wrong number is not worth 9%.
- **A `Thread.current`-keyed buffer.**
  Safe, and the placement the thread-safety constraint suggests first, but the lookup costs about as much as the allocation it removes: between 3% faster and 5% slower than the `pack` baseline across runs, so it buys nothing.
- **Reading the scratch through a method call instead of the inline `@scratch`.**
  Half the win (12.6% against 25.3%): the call costs as much as the conversion it guards.
- **A fresh `IO::Buffer` per call.**
  No shared state at all, but allocating the buffer costs more than allocating the String did.

## Consequences

- Positive: the conversions no longer allocate, so a float-heavy guest stops pressuring the GC; the micro benchmark drops 25% of its wall time and 93% of its allocations.
- Negative: `Rt::Memory` includes `Rt`, so the two namespaces must stay collision-free, and the memory units' float calls are now bare, which the units lint's `Rt.` rule no longer sees (their `# requires:` lines are maintained by hand, as the lint's other unmatched shapes already are).
- Carry-over: the NaN payload an f32 arithmetic result carries changed, see [decision 2](2-numeric-semantics.md)'s revision.

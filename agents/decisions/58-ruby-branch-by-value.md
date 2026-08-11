# Decision 58: Ruby Backend Addresses a Branch by Value, Not by Lexical Scope

Status: **Accepted, 2026-08-03.**
Implemented in the `flat` module of [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs).
Supersedes [decision 42](42-ruby-label-variable-cascade.md)'s cross-frame relay protocol, which no longer runs on any path; decision 42's lean frame shapes and depth-1 fast path stand and remain the lowering for every function with no crossed frame, 45% of `sqlite3-shell`'s.
State merging is deliberately not included; see Consequences.
**Refined by [decision 60](60-ruby-flatten-only-deep-crossings.md) (2026-08-04):** the relay runs again for branches crossing fewer than 16 frames: only deep crossings are addressed by state, after the dispatch probe showed up as 11.8% of the NES workload's wall time.

## Context

Decision 42 addresses a branch target by its *scope*: `break` leaves the innermost one, so naming a target further out sets `__br` and relays through every frame in between, one epilogue compare per level.
That is linear in nesting depth.
On the converted `sqlite3-shell` it produced 33,219 `__br` references and roughly half of all CPU, concentrated in the VDBE interpreter loop where the towers are deepest.

Two existing wasm-to-C compilers avoid the problem structurally.
w2c2 emits `goto label_N`; WasmKit emits `pc += offset`.
Both name the target as a *value*, so a branch costs the same from any depth.
Ruby's equivalent primitive is `opt_case_dispatch`: a `case` over integer literals compiles to a single hash probe with no compares.

The obvious alternative (put each state in its own method) is closed off here: neither YJIT nor ZJIT implements on-stack replacement, so a hot loop inside a once-called method is never compiled.
Measured across nojit/yjit/zjit it runs 1.611 / 1.614 / 1.608 s, identical.
Any scheme that moves work across a method boundary pays a call and gains no compilation.

## Decision

- **A function with cross-frame branches becomes a dispatch loop.**
  `state = 0` then `while true / case state`, each dissolved frame's landing point an integer state, each branch `state = N; next`.
  The relay stops existing rather than getting cheaper.
- **Only crossed frames dissolve, plus transitively their ancestors.**
  Both `begin ... end while false` and `while true` are Ruby loops, so any frame a branch escapes must stop being one or it captures the `next` aimed at the dispatch loop; and so must anything containing such a frame.
- **The discriminating criterion is: flatten branches, not loops.**
  A frame no branch escapes keeps its structured form.
  This is not economy: turning a tight back-edge into a state transition measured *slower* than the cascade it replaces once the loop runs ~100 trips per entry.
- **One outward shape is exempt: Ruby's `break`.**
  A `br` crossing a single loop that is the **sole** statement of the block it targets leaves that loop and lands at the block's end, where the branch was going, in O(1).
  That is the standard compilation of `while` with a conditional exit, and dissolving it is what regressed the microbenchmarks.
  *Sole*, not merely *last*, is load-bearing: any other statement in the block could dissolve, which dissolves the block by the ancestor rule while the loop stays a Ruby `while`, and a `state = N; next` is then captured by that `while`.
  Requiring the loop to be the whole body makes the two dissolve together or not at all.
- **`__br` is not hoisted in a flattened function**, and a frame's exit transition is not emitted when the body already ended in a `br`, `return` or `unreachable` (`terminates`).
  Both are asked of the IR, never recovered from the emitted text.

## Rejected alternatives

- **Keep the decision 42 cascade.**
  Linear in depth by construction; 33,219 `__br` references and ~half of CPU on the workload that motivated this.
- **Flatten loops too, for uniformity.**
  Loses to the cascade outright past ~100 trips per entry: the back-edge is the one place the structured form is cheaper.
  This is the criterion above, stated as its own rejection.
- **State the `break` exemption as "the loop is the last statement".**
  Incorrect, and not merely conservative: it fires on 374 sites in `sqlite3-shell` where the block holds something else, and the generated program's output diverges.
  Caught by the byte-comparison test, not by the microbenchmarks, all of which have the sole-statement shape.
- **Clean the emitted state bodies with a text pass.**
  Recovers a control-flow graph the IR already has, by `strip_prefix("state = ")` over generated Ruby.
  Its dead-code half measures 0% (unreachable code does not run), and its state-merging half measured 1.5-4%, inside this host's drift.
  Structure it in the IR or not at all.
- **Per-state outlining, or block outlining into methods.**
  1.15-1.18x *slower* at every test; hot states are 5-10 lines, too small to carry a call.
  See the no-OSR measurement in Context.

## Consequences

**Positive.**
2.08 ± 0.06x on `sqlite3-shell` (`w_cpu_50000`, Ruby 4.1.0dev `--zjit`), output byte-identical.
The relay protocol is gone: 33,219 `__br` references to zero.
Generated output is 5,989 lines *smaller* than the cascade's.
The `break` exemption also improves the cascade path: the shape it recognises now emits a plain `break` instead of `__br = N; break` plus a relay epilogue, so the wat microbenchmarks come out slightly ahead of the pre-flattening lowering rather than behind it.

**Negative.**
Depth-1 branches get *more* expensive.
They are the majority of all branches (20,588 vs 19,939 outward in `sqlite3-shell`), each a single `break` under decision 42; 69% of those in flattened functions (89% in `ruby.wasm`) now pay an assignment plus a hash dispatch instead, because their target frame was dissolved by some *other*, deeper branch.
This is the trade the design makes: the deep case gets much cheaper, the shallow case somewhat dearer, and on real modules the deep case dominates.
It is also why the criterion above matters: every frame kept structured is depth-1 branches kept cheap.

**Carry-over.**
A state whose only entry is one predecessor's trailing transition is that predecessor's continuation and could be spliced in, removing a dispatch round-trip.
Measured at 1.5-4%, which is inside the measurement drift of the host it was taken on; it is not implemented here and needs an interleaved measurement before it is.

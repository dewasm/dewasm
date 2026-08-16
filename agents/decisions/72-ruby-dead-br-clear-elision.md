# Decision 72: Ruby Backend Omits Dead Method-Level `__br` Clears

Status: **Accepted, 2026-08-15.**
Implemented as `dead_clears` in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs); refines the method-body-level spelling of [decision 42](42-ruby-label-variable-cascade.md)'s land-or-relay epilogue.
The relaying spelling inside an enclosing frame is untouched.

## Context

Decision 42's epilogue has two spellings: inside an enclosing frame it lands or relays, and at method-body level, where nothing outer exists to relay to, it degenerates to `__br = nil if __br == {id}`.
At that spelling a pending `__br` can only name the frame itself: an outer frame a branch could target would be on that branch's inclusive path, and [decision 60](60-ruby-flatten-only-deep-crossings.md)'s dissolution is all-or-nothing per path, so no surviving frame has a dissolved lexical ancestor a branch still relays toward.
The statement therefore never redirects control; it only resets `__br` for later reads, and where nothing later reads `__br` it is dead text.
Converted `ruby.wasm` carried 9,174 such clears, 97% of them dead (issue #222).

## Decision

Ask liveness of the IR before emission, never of the emitted text: a pre-pass walks the function body backward in emission order and drops a method-level clear when no `__br` read (a surviving crossed frame's epilogue, a wrapped-loop head check, a post-loop relay) can execute after it.
Emission order equals execution order for the structured lowering, so "after" is exactly the backward walk's remainder.
A dissolved loop breaks that equation, its back-edge re-running reads that precede the clear, so nothing under one is ever dropped.
When in doubt the clear stays: the read test over-approximates (every surviving crossed frame counts, though a crossed loop at method-body level emits no post-loop relay), and statements whose emission context the walk does not model are skipped, which can only keep a droppable clear.

## Rejected alternatives

- **Keep emitting every clear.**
  The clear is the most repeated single line in large outputs: 928 in `sqlite3-shell`, 9,174 in `ruby.wasm`, most with nothing left to protect.
- **Full liveness over the dispatch-state graph.**
  Would additionally drop clears under dissolved loops whose bodies read nothing, but needs per-state reachability; the emission-order rule already removes 71% (`sqlite3-shell`) to 97% (`ruby.wasm`) of the clears, and the remainder does not justify a second flow analysis to keep correct.
- **A text post-pass over the generated Ruby.**
  Recovers control flow the IR already has; rejected on the same ground decision 58 rejected its cleanup pass: structure it in the IR or not at all.

## Consequences

- `sqlite3-shell`: 928 → 267 clears, 17,323 bytes (0.22%) smaller; `ruby.wasm`: 9,174 → 239, 232,477 bytes (0.32%) smaller.
  No semantic change: the diffs are purely removed clear lines, and the spec harness and the slow app set pass unchanged.
- The symmetric refinement is not taken: the relaying spelling's land arm can likewise be dead, but the relay arm is protocol-required and the spelling is a single line either way.
- The codegen-shape tests pin both sides and the conservatism: `dead_method_level_clear_is_dropped`, `method_level_clear_before_a_later_reader_is_kept`, `method_level_clear_under_a_dissolved_loop_is_kept` in `crates/dewasm-backend-ruby/src/lib.rs`.

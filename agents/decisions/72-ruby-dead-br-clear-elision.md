# Decision 72: Ruby Backend Drops Dead Method-Level `__br` Clears

Status: **Accepted, 2026-08-15.**
Implemented as `dead_clears` in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs).
It refines the method-body-level spelling of [decision 42](42-ruby-label-variable-chain.md)'s land-or-relay epilogue.
The relaying spelling inside an outer frame is untouched.

## Context

Decision 42's epilogue has two spellings.
Inside an outer frame it lands or relays.
At method-body level nothing outer exists to relay to.
So it reduces to `__br = nil if __br == {id}`.
At that spelling a pending `__br` can only name the frame itself.
That is because an outer frame a branch could target is on that branch's path, both ends included.
[Decision 60](60-ruby-flatten-only-deep-crossings.md) dissolves frames all-or-nothing per path.
So no surviving frame has a dissolved lexical ancestor a branch still relays toward.
The statement therefore never redirects control; it only resets `__br` for later reads.
Where nothing later reads `__br`, it is dead text.
Converted `ruby.wasm` carried 9,174 such clears, 97% of them dead (issue #222).

## Decision

Ask liveness of the IR before emission, never of the emitted text.
A pre-pass walks the function body backward in emission order.
It drops a method-level clear when no `__br` read can execute after it.
Such a read is one of these:

- a surviving crossed frame's epilogue;
- a wrapped-loop head check;
- a post-loop relay.

Emission order equals execution order for the structured lowering.
So "after" is exactly the backward walk's remainder.
A dissolved loop breaks that equation: its back-edge re-runs reads that precede the clear.
So nothing under a dissolved loop is ever dropped.
When in doubt the clear stays.
The read test over-approximates: every surviving crossed frame counts.
Yet a crossed loop at method-body level emits no post-loop relay.
Statements whose emission context the walk does not model are skipped.
Skipping can only keep a droppable clear.

## Rejected alternatives

- **Keep emitting every clear.**
  The clear is the most repeated single line in large outputs.
  There are 928 in `sqlite3-shell` and 9,174 in `ruby.wasm`, most with nothing left to protect.
- **Full liveness over the dispatch-state graph.**
  Would additionally drop clears under dissolved loops whose bodies read nothing.
  But it needs per-state reachability.
  The emission-order rule already removes 71% (`sqlite3-shell`) to 97% (`ruby.wasm`) of the clears.
  The remainder does not justify a second flow analysis to keep correct.
- **A text post-pass over the generated Ruby.**
  It recovers control flow the IR already has.
  It is rejected on the ground decision 58 rejected its own text post-pass.
  That ground: structure it in the IR or not at all.

## Consequences

- `sqlite3-shell`: 928 → 267 clears, 17,323 bytes (0.22%) smaller.
  `ruby.wasm`: 9,174 → 239, 232,477 bytes (0.32%) smaller.
  No semantic change: the diffs are purely removed clear lines.
  The specification harness and the slow app set pass unchanged.
- The matching refinement of the other spelling is not taken.
  The relaying spelling's land arm can likewise be dead.
  But the relay arm is protocol-required, and the spelling is a single line either way.
- Tests of the generated code's shape check both sides and the conservative choice.
  They also check the cases where the rule keeps a clear it could drop.
  They are in `crates/dewasm-backend-ruby/src/lib.rs`:
  - `dead_method_level_clear_is_dropped`;
  - `method_level_clear_before_a_later_reader_is_kept`;
  - `method_level_clear_under_a_dissolved_loop_is_kept`.

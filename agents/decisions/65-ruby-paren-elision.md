# Decision 65: Precedence-Aware Parenthesis Emission in the Ruby Backend

**Status:** Accepted (2026-08-06).
Landed: the `Prec`/`Rendered` pair in `crates/dewasm-backend-ruby/src/lib.rs`.
Every expression the Ruby backend emits now goes through it.
Not covered: the other backends.
They keep parentheses around every expression until each one gets the same treatment.
That treatment is against its own language's table.

## Context

The Ruby backend put every expression node it built in parentheses, unconditionally.
Examples are `((l0 + 1) & 0xffffffff)`, `(a ? 1 : 0)`, `(a >> (b & 31))`.
That is the cheapest way to be certainly correct while lowering, and it was never revisited.

It is not free.
On the converted `ruby.wasm` artifact, grouping parentheses are 342k pairs.
They are 12.2% of the Prism nodes the file parses into.
Each pair is a `ParenthesesNode` plus a `StatementsNode`.
Loading such a file is almost entirely parse and compile.
So those nodes are paid for on every `require`.
They emit no instructions, so nothing is bought with them.
The retained ISeq is identical either way.

## Decision

**Each rendered expression carries the precedence it binds at, and a context asks for it at a limit.**
`Rendered { src, prec, op }` replaces the plain `String` the renderers passed around.
`Rendered::at(limit)` returns the text in parentheses only when it binds looser than the limit.
`free()` is the limit of a position that constrains nothing.
Such a position is one of these:

- a statement's right-hand side;
- an `if` condition;
- a call argument;
- an array element.

The parentheses are therefore emitted by the *consumer* of an expression.
It is the only place that knows whether they are needed.

**The precedence table covers only the subset the backend emits.**
It is written out rather than assumed, because two of its facts differ from C.
Ruby binds `&` tighter than `|` and `^`.
It binds all three tighter than the comparisons.
So `a + b & 0xffffffff` is the i32 wrap, correctly, and `a & b | c` is `(a & b) | c`.

**The table is conservative wherever Ruby's grammar is not plainly on our side.**
The reason is that [decision 1](1-ir-design.md) puts correctness of generated code above its readability.
This change buys only bytes.
The conservative rules are these:

- The comparison and equality families are non-associative.
  (`a == b == c` is a syntax error in Ruby.)
  So an equal-precedence operand is put in parentheses on *both* sides.
- A left operand at the operator's own precedence is left without parentheses.
  Left associativity reparses it as built.
  A right operand at the same precedence keeps its parentheses.
  The exception is `& | ^` with the identical operator.
  There, integer associativity makes the flattening exact.
  `+` and `*` are excluded from that exception on purpose.
  They carry floats here, and float addition is not associative.
- A negative numeric literal binds like a unary expression, not like an atom.
  So it is put in parentheses in receiver position.
- The ternary is right-associative: only its else-branch may hold another one without parentheses.
- `!` keeps the parentheses around a comparison (`!(a < b)`), which the table requires anyway.
  [Decision 2](2-numeric-semantics.md)'s NaN handling depends on them.
  `!` wraps a whole comparison; the operator is never flipped instead.

**The condition renderers go through the same mechanism.**
`cond`/`not_cond` fuse a wasm comparison into a Ruby `true` or `false`.
They previously emitted their operands with a different rule for parentheses.
It differed from that of the materialized `a < b ? 1 : 0` form.
The same expression got parentheses in one path and not in the other.
With one table there is one answer.

## Rejected alternatives

**Keep parentheses around every expression.**
The current state, and it is measurably not free.
It costs 12.2% of the parsed nodes and about 6% of the load time of a large artifact.
That text compiles to nothing.

**Remove unneeded parentheses in a pass over the emitted text.**
It would leave the emitter alone.
But removing a parenthesis correctly requires knowing what the text around it parses as.
So the pass has to carry the whole grammar.
It repeats knowledge the emitter already has in structured form.
It applies that knowledge to the one representation where it is hardest to be sure of.
The emitter knows the tree; the text does not.

**Exploit associativity and operator flipping aggressively.**
That covers these rewrites:

- rewriting `a - (b - c)`;
- flattening float chains;
- dropping parentheses by reasoning about operand ranges.

Each is a further few bytes and a new way to be wrong.
The cost of one retained pair is bytes.
The cost of one wrong elision is a semantics bug the specification harness might or might not reach.
Where the table has no clear answer, the parentheses stay.

## Consequences

- Converted source gets 1.5-2.6% smaller, and the `(` count falls by a third to a half.
  `compile_file` gets 2-5% faster (medians of three runs).

  | App | `(` count | `compile_file` |
  | --- | --- | --- |
  | `cowsay` | 54.8k → 26.7k | 0.119s → 0.113s |
  | `qjs` | — | 0.434s → 0.413s |
  | `sqlite3-shell` | 204k → 123k | 0.526s → 0.508s |
  | ruby.wasm | 1.60M → 1.05M | 4.88s → 4.79s |

- Generated Ruby now relies on the reader knowing Ruby's precedence.
  `s32(a) >> (b & 31) & 0xffffffff` is a correct i32 arithmetic shift.
  It does not look like one at a glance.
  That is the readability-for-correctness trade [decision 1](1-ir-design.md) already settled this way.
  The specification harness ([decision 3](3-testing-strategy.md)) is what says the shapes are right.
- The other backends still put parentheses around every expression.
  Each follows in its own change, against its own language's table.
  The differences that matter here are exactly the ones that differ per language.
  Those are `&` versus comparisons, and non-associative equality.
  So a shared framework would have to take as input everything that makes it correct.

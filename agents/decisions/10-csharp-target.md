# Decision 10: Add C# to the Target Languages, Paired with Java

Status: **Accepted, 2026-07-23.**
Changes decision 0's target list; no backend work has started.
Planned order: Ruby → Bash → Java → C# → Go → Python → PHP.
Java and C# are designed as one "managed static languages" pair.
*Revised by [decision 24](24-01-scope-reset.md) (2026-07-25): the 0.1 backends are Python, Go, and Java.*
*C# moves to the future list.*
*The Java/C# pairing argument below still applies when C# is picked up.*

## Context

C# was missing from the original target list (the user pointed it out).
It fits the decision 0 criterion.
C# is a widely used language whose tools and libraries do not ship a wasm runtime by default.
That holds in the places `dewasmify` targets.
And no wasm→C# *source* converter exists.

## Decision

Add C#, scheduled together with Java.
The two backends share almost all design decisions.
Examples are a class-shaped module, byte-array linear memory, and exceptions for traps.
So the marginal cost of the second one is small.
Where they differ, C# is the easier half:

- Native unsigned integers (`uint`/`ulong`).
  So decision 2's masked-unsigned strategy is not needed.
- `goto` for multi-level `br`.
- `Span<byte>`/`BinaryPrimitives` for little-endian memory access.
- No hard method-size limit like the JVM's 64 KB.
  Java keeps its function-splitting task.

A shared lowering-conventions decision for the pair is expected when that milestone starts.

## Rejected alternatives

- **Not adding it**: leaving it out was a mistake, not a decision.
- **Revisiting JavaScript on the same grounds**: unchanged from decision 0.
  wasm2js exists, and every JS runtime ships a wasm engine.

## Consequences

- The README target table and the roadmap gain C#.
  The support matrix (`docs/support.md`) grows a column when the backend lands.
- The Java/C# milestone produces one design and two emitters.
  It is a first test of how much backend machinery is reusable across similar languages.
  Examples are decision 6 units and lowering tables.

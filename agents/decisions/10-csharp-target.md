# Decision 10: Add C# to the Target Languages, Paired with Java

Status: **Accepted, 2026-07-23.**
Amends decision 0's target list; no backend work has started.
Planned order: Ruby → Bash → Java → C# → Go → Python → PHP, with Java and C# designed as one "managed static languages" pair.
*Revised by [decision 24](24-01-scope-reset.md) (2026-07-25): the 0.1 backends are Python, Go, and Java; C# moves to the future list (the Java/C# pairing argument below still applies when C# is picked up).*

## Context

C# was simply missing from the original target list (user call-out).
It fits the decision 0 criterion (a mainstream language whose ecosystem does not ship a wasm runtime by default in the places dewasmify targets), and no wasm→C# *source* translator exists.

## Decision

Add C#, scheduled together with Java: the two backends share almost all design decisions (class-shaped module, byte-array linear memory, exceptions for traps), so the marginal cost of the second one is small.
Where they differ, C# is the easier half: native unsigned integers (`uint`/`ulong`, so decision 2's masked-unsigned strategy is bypassed entirely), `goto` for multi-level `br`, `Span<byte>`/`BinaryPrimitives` for little-endian memory access, and no hard method-size limit like the JVM's 64 KB (Java keeps its function-splitting task).
A shared lowering-conventions decision for the pair is expected when that milestone starts.

## Rejected alternatives

- **Not adding it**: the omission was an oversight, not a decision.
- **Revisiting JavaScript on the same grounds**: unchanged from decision 0: wasm2js exists and every JS runtime ships a wasm engine.

## Consequences

- README target table and roadmap gain C#; the support matrix (docs/support.md) grows a column when the backend lands.
- The Java/C# milestone produces one design, two emitters, a first test of how much backend machinery (decision 6 units, lowering tables) is reusable across similar languages.

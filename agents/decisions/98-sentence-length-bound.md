# Decision 98: A Sentence Fits in 100 Columns

Status: **Accepted, 2026-09-28.**
Landed: [`AGENTS.md`](../../AGENTS.md) states the bound in its Writing style section and meets it.
The rest of the prose predates the bound.
That is `agents/`, `docs/`, the README, the Rust doc comments, and the xtask templates.
A dedicated pass brings them under the bound, and a mechanical check then keeps them there.

## Context

Prose here is written one sentence per line, with no line wrapping.
That made sentence length visible, and nothing bounded it.
`awk` counted `agents/`, `docs/` and the README, excluding table rows and generated results.
2,826 lines ran past 100 columns, and 2,102 of them past 140, while 25 lines held two sentences.
The rule was followed, and the sentences were long.

The sibling project dewasm/cowsay.wasm used the same rules plus a 100-column bound.
It applied them to its README, its specification, and its C comments.
The bound changed what survived a rewrite.
Modifiers went ("byte for byte", "in plain sight"), idioms went, and a two-thing sentence was split.

## Decision

A sentence fits in 100 columns, and a line is a sentence, so a long line is the defect it shows.
The remedies come in a fixed order.
First remove words that add no information.
If the sentence is still too long, it states two things: split it at a sentence or at a `;`.
Connectives stay, and a table row is exempt, since Markdown puts a row on one line.

The bound works with the vocabulary rules, and only with them.
Under a bound, a writer has four ways out that do not improve the sentence.
They are: wrap the line, reach for a metaphor, swap in a weaker word, drop the connectives.
Sentence-per-line blocks the first, and the ban on coined metaphors the second.
One term per concept blocks the third, and the connectives rule the fourth.
What remains is to remove words that carry nothing, or to split.

## Rejected alternatives

- **No bound**, the status quo.
  Sentence-per-line makes length visible but applies no pressure; the count above is the result.
- **Wrap at a fixed column.**
  A wrap hides sentence length inside a paragraph, which is why sentence-per-line was adopted.
  The two cannot combine.
- **A soft bound, "about 100".**
  It cannot be checked mechanically.
  In cowsay.wasm the check, `awk 'length($0) > 100'`, is what kept the rule alive.
- **Count rendered text rather than raw columns.**
  A Markdown link doubles a phrase's length, and raw columns are what a check can count.
  A link that will not fit moves to a sentence of its own.

## Consequences

- Positive: a sentence that meets the bound under the vocabulary rules says one thing plainly.
  The rules select for the sentences worth keeping.
- Negative: existing prose does not meet the bound.
  Bringing it under is a pass over every document, checking each sentence's meaning as it shortens.
- Carry-over: that pass adds a check to `cargo test -p xtask`.
  It checks 100 columns and one sentence per line, with table rows exempt.
  Until it lands, text is brought under the rules when edited, not in passing.

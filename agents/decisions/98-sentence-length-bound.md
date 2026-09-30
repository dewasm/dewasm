# Decision 98: A Sentence Is at Most 100 Characters as Read

Status: **Accepted, 2026-09-28.**
Landed: [`AGENTS.md`](../../AGENTS.md) states the bound in its Writing style section and meets it.
The measure was settled on 2026-09-30: characters as read, plus a line bound in source code.
The rest of the prose predates the bound.
That is `agents/`, `docs/`, the README, the comments in source code, and the xtask templates.
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

Two measurements decided what the bound counts.
A link target spends columns without adding a word: 46 lines passed 100 columns on targets alone.
In 3 of them a single link was longer than 100 columns, so no wording could meet a raw bound.
A comment in source code spends columns on its prefix, the indentation and the marker.
The prefix is at most 8 columns for 82% of the 4,408 comment lines, and more than 20 for 1.6%.

## Decision

A sentence is at most 100 characters as read.
Markup, link targets, indentation, and list markers do not count, since none of them is a word.
In source code the line also fits in 100 columns, prefix included, which is rustfmt's `max_width`.
A comment then never runs wider than the code around it, and a deeper comment is a shorter sentence.
Markdown has no line bound: its lines were never bounded, and a long target sits at the end of one.
A table row is exempt, since Markdown puts a row on one line.

The remedies come in a fixed order.
First remove words that add no information.
If the sentence is still too long, it states two things: split it at a sentence or at a `;`.
Connectives stay.

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
- **Raw columns everywhere.**
  The bound presses on the words of a sentence, and a link target is not one.
  Under it, a sentence next to a link was bent into a label and a colon to fit.
- **A line bound in Markdown as well.**
  It needs reference-style links wherever a target is long: 14 of the 65 link lines that fit as read.
  That is one more convention, for lines nobody bounded before.
- **120 columns in source code.**
  Nearly every comment would keep the full 100 characters, and would run 20 columns past the code.

## Consequences

- Positive: a sentence that meets the bound under the vocabulary rules says one thing plainly.
  The rules select for the sentences worth keeping.
- Negative: existing prose does not meet the bound.
  Bringing it under is a pass over every document, checking each sentence's meaning as it shortens.
- Negative: counting characters as read needs a Markdown parser in the check.
  A plain substitution miscounts a `*` or `_` inside a code span.
- Carry-over: that pass adds two checks to `cargo test -p xtask`.
  One counts a Markdown sentence as read, with table rows and fenced code exempt.
  The other counts the columns of a comment line in source code.
  Until they land, text is brought under the rules when edited, not in passing.

# Decision 98: A Sentence Is at Most 100 Characters as Read

Status: **Accepted, 2026-09-28.**
Landed: [`AGENTS.md`](../../AGENTS.md) states the rules in its Writing style section and meets them.
The measure and the clause rule were settled on 2026-09-30.
The pass in #341 brought the rest of the text under the bound on 2026-09-30.
That is `agents/`, `docs/`, the README files, the comments in source code, and the `xtask` templates.
`cargo test -p text-check` checks every tracked text file; `cargo xtask check-text` reports one.
The `lint` job runs it, so a defect fails CI without waiting for the other jobs.

## Context

Text here is written one sentence per line, with no line wrapping.
That made sentence length visible, and nothing bounded it.
`awk` counted `agents/`, `docs/` and the README, excluding table rows and generated results.
2,826 lines ran past 100 columns, and 2,102 of them past 140.
Only 25 lines held two sentences.
The rule was followed, and the sentences were long.

The related project dewasm/cowsay.wasm used the same rules plus a 100-column bound.
It applied them to its README, its specification, and its C comments.
The bound changed what survived a rewrite.
Modifiers and idioms went ("byte for byte", "in plain sight"), and a two-thing sentence was split.

The rules are not new, and the norms below show what a complete set holds.

| Norm | What it sets |
| --- | --- |
| Kernighan, [UNIX for Beginners](https://rhodesmill.org/brandon/2012/one-sentence-per-line/) (1974) | Start each sentence on a new line; break at commas and semicolons. |
| [Semantic Line Breaks](https://sembr.org/) | Break after a sentence; 80 characters recommended; a line may pass it for links, code, or markup. |
| [ASD-STE100](https://www.asd-europe.org/standards-specifications/simplified-technical-english/), Simplified Technical English | 20 words in a procedure, 25 in a description; one meaning per word. |
| [GOV.UK](https://insidegovuk.blog.gov.uk/2014/08/04/sentence-length-why-25-words-is-our-limit/) writing guide | Split a sentence over 25 words. |
| Oxford Guide to Plain English | 15 to 20 words a sentence on average. |
| [Simple English Wikipedia](https://simple.wikipedia.org/wiki/Wikipedia:How_to_write_Simple_English_pages) | At most one subordinate clause; no idioms; no length in numbers. |

STE was written for readers whose first language is not English.
It pairs a fixed vocabulary with a bound on the sentence, which is the pairing adopted here.
Two parts of that set were already in place.
Sentence-per-line is Kernighan's, and one term per concept is STE's.

Two measurements decided what the bound counts.
A link target spends columns without adding a word: 46 lines passed 100 columns on targets alone.
In 3 of them a single link was longer than 100 columns, so no wording could meet a raw bound.
A comment in source code spends columns on its prefix, the indentation and the marker.
The prefix is at most 8 columns for 82% of the 4,408 comment lines, and more than 20 for 1.6%.

## Decision

A sentence is at most 100 characters as read.
Markup, link targets, indentation, and list markers do not count, since none of them is a word.
In source code the line also fits in 100 columns, prefix included: the `max_width` of `rustfmt`.
A comment then never runs wider than the code around it, and a deeper comment is a shorter sentence.
Markdown has no line bound.
Its lines were never bounded, and a long target sits at the end of one.
The bound does not apply to a table row, since Markdown puts a row on one line.

The norms count words, and this bound counts characters.
A check counts characters without defining a word, and source code already measures in columns.
The aim is still the norms' aim, a sentence of about 16 words.
`AGENTS.md` runs 6.3 characters a word with its space, so 100 characters hold about 16 words.
That is the Plain English average and under STE's 20, and an identifier lowers the count further.
A sentence that fits by an abbreviation or a dropped article has missed the aim.

A sentence holds at most two clauses, joined once by a linking word, a `;`, or a `:`.
A third clause starts a new sentence.
A clause that only identifies a noun ("the spelling that compiles") does not count.
This is Simple English Wikipedia's limit on subordinate clauses, extended to every joint.
The length bound cannot replace it: 100 characters are room enough for three nested clauses.
No check parses clauses, so this rule is kept by reading.
The length bound is kept by the check.

The fixes for a long sentence come in a fixed order.
First remove words that add no information.
A sentence still too long states two things: split it at a sentence or at a `;`.
Linking words stay.

The bound works with the vocabulary rules, and only with them.
Under a bound, a writer has four ways out that do not improve the sentence.
They are: wrap the line, reach for a metaphor, swap in a weaker word, drop the linking words.
Sentence-per-line blocks the first, and the ban on coined metaphors the second.
One term per concept blocks the third, and the rule on linking words the fourth.
What remains is to remove words that carry nothing, or to split.

## Rejected alternatives

- **No bound**, the current state.
  Sentence-per-line makes length visible but applies no pressure; the count above is the result.
- **Wrap at a fixed column.**
  A wrap hides sentence length inside a paragraph, which is why sentence-per-line was adopted.
  The two cannot combine.
- **A soft bound, "about 100".**
  It cannot be checked mechanically.
  In cowsay.wasm the check, `awk 'length($0) > 100'`, is what kept the rule alive.
- **Count words, as the norms do.**
  A word count needs a definition of a word for a path, an identifier, and a code span.
  A character count needs none, and source code already counts columns.
- **Raw columns everywhere.**
  The bound presses on the words of a sentence, and a link target is not one.
  Under it, a sentence next to a link was bent into a label and a colon to fit.
- **A line bound in Markdown as well.**
  It needs reference-style links wherever a target is long: 14 of the 65 link lines that fit as read.
  That is one more convention, for lines nobody bounded before.
- **120 columns in source code.**
  Nearly every comment would keep the full 100 characters, and would run 20 columns past the code.
- **A clause rule alone**, as Simple English Wikipedia has.
  Nothing in it can be checked, and a check is what kept the length rule alive.
- **An existing text linter for the checks**, Vale or textlint.
  Each splits text into sentences itself, and each split wrongly on text that meets the rules.
  Vale joined two lines at `docs/testing.md.`, and textlint at a bold label that ends in a period.
  Here a line is a sentence, so a check that counts lines has nothing to split.
  A count of linking words per sentence, tried in Vale for the clause rule, gave only false positives.

## Consequences

- Positive: a sentence that meets the bound under the vocabulary rules says one thing plainly.
  The rules select for the sentences worth keeping.
- Positive: the rules rest on norms written for readers of English as a second language.
  A future change can be weighed against those norms, not against taste.
- Negative: existing text does not meet the bound.
  Bringing it under is a pass over every document, checking each sentence's meaning as it shortens.
- Negative: counting characters as read needs a Markdown parser in the check.
  A plain substitution miscounts a `*` or `_` inside a code span.
- Carry-over: that pass added two checks, now in `cargo test -p text-check`.
  One counts a Markdown sentence as read, and skips table rows and fenced code.
  Another counts the columns of a comment line in source code.
  A third, still to come, reports a use of a word listed in [`agents/vocabulary.md`](../vocabulary.md).
  That list is a table, so a linter's rule files can be generated from it later.
  No check detects a sentence wrapped across lines, or a third clause; a reader checks those.

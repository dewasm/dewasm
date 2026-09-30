# Decision 99: Text Uses a Learner Word List Plus Project Terms

Status: **Accepted, 2026-09-30.**
Landed: [`agents/vocabulary.md`](../vocabulary.md) holds the sources of allowed words and the excluded words.
`cargo test -p text-check` applies the lists to every tracked Markdown file and source comment.
All of them pass.
The pass in #341 rewrote the Markdown written before the lists, and the pass in #355 the comments.
A comment a program reads, such as a unit's `# requires:` header, is not text and is not checked.

## Context

The writing rules exclude coined metaphors, second names for one concept, and intensifiers.
A list of excluded words can only hold the words someone has already noticed.
Such a list existed only in one agent's session notes, and whole-word counts showed what it missed.
`lane`, `gated` and `tiered` were each in the repository.
One of them was written by the agent that held the list.

The readers these rules serve read English as a second language (decision 98).
The norms for such readers limit the words as well as the sentence.
ASD-STE100 approves 875 words, and each organization adds its own technical names and verbs.

A measurement compared four base lists.
spaCy reduced each word to its base word, and only content words were matched.
The text measured was the three files already under the rules: 3,174 words, 619 base words.

| Base list | Words covered | Base words outside |
| --- | --- | --- |
| Basic English 850 + VOA Special English 1500 | 77.5% | 342 |
| NGSL, 2,809 words | 89.1% | 194 |
| NGSL + NAWL, 957 more | 92.0% | 152 |
| NGSL + NAWL + CSAVL, a computer science list | 93.3% | 132 |

Each of the 132 was then read in its sentence.

| Kind | Base words |
| --- | --- |
| Term of the field | 49 |
| Name, abbreviation, or a wrong part of speech from the tool | 25 |
| Derived form of a listed word | 24 |
| Word with a plainer replacement | 22 |
| Term for writing | 12 |

The 22 held words no excluded list had caught: `sibling`, `pin`, `outrank`, `status quo`, `verbatim`.
One of them, `companion`, was the replacement the excluded list itself offered for `sidecar`.

## Decision

A word in a sentence comes from a base list, a derived form, a project term, a name, or a number.
The base list is NGSL 1.2 plus NAWL 1.2, since both were made for learners and state a license.
A word from none of those sources gets a plainer replacement, or joins the project terms.
The test for a term of the field stays: a reader who looks it up reaches an official document.

A command, a path, or an identifier goes in a code span, and a code span is not checked.
So a short form such as `spec` or `docs` needs no entry, and a sentence writes the full word.
The terms for writing are allowed only in the documents about writing.
Elsewhere a use of `subordinate` or `idiom` would most likely be a metaphor.

A project term can be held to one meaning, as ASD-STE100 holds each approved word.
`temp` names a variable of the IR, and it never shortens "temporary".
A short form is allowed as the name of one thing, not as a shorter way to write a longer word.
Of 170 uses of `temp` in text and comments, 17 meant a file or a directory.
Those 17 are now excluded.

The excluded words stay, and they apply on top of every source.
The base list itself holds `gate`, `green`, `sweep`, and `wire`, because a metaphor is made of plain words.
The two kinds of list catch different things: one the known metaphors, the other each new word.

## Rejected alternatives

- **Excluded words only.**
  It cannot report a word nobody has listed, and the 22 replacements above are what it missed.
- **Basic English or VOA Special English as the base.**
  They leave out `code`, `check`, `file`, and `length`, so the project terms would hold most of the language.
- **A list by frequency on the web.**
  Its first 5,000 base words leave 160 outside and let in `lane` and `hood`.
  Nobody chose its words for learners.
- **CSAVL on top.**
  It moves 20 base words such as `compile` and `runtime` into the base, and states no license.
  Those 20 are project terms instead.
- **The ASD-STE100 dictionary.**
  It approves a word with one part of speech and one meaning, which a match on spelling cannot check.
  A tool that marks parts of speech found 13 of the 41 uses of `wire` as a verb.
  Its words were also chosen for the maintenance of aircraft.
- **The word lists of a linter.**
  Vale has none of its own, and the lists in its style packages are excluded words, as ours are.

## Consequences

- Positive: a new word shows at the moment it enters, as one line added to a table in review.
- Positive: a general word outside the base list is a reason to look for a plainer one.
- Negative: the project terms are a list to keep, and it grows with the text it covers.
- Negative: a check needs the base word of each word, so it needs the rules of derived forms in code.
- Carry-over: the passes of decision 98 added the check after the text was rewritten.
  Before them, 1,679 base words of the Markdown fell outside NGSL and NAWL.
  The comments held 1,635 more distinct words outside the lists.
  The repository does not store the two base lists, which are under CC BY-SA 4.0.
  `crates/text-check/setup.sh` fetches them by sha256 and names their authors.
  The `lint` job keeps them in its cache.

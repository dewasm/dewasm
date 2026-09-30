# Vocabulary

Which words this project writes in a sentence, and which it does not.
`AGENTS.md` states the rules: no coined metaphor, one term per concept, facts over intensifiers.
This file holds the lists those rules work from (decision 99).

## Allowed words

A word in a sentence comes from one of the sources below.

| Source | What it holds |
| --- | --- |
| Base list | NGSL and NAWL, two word lists made for learners of English: 3,766 headwords. |
| Derived form | A base word or a project term with `un-`, `re-`, `mis-` or `non-` in front, or with `-ly`, `-ness`, `-er`, `-able`, `-ion`, `-ity`, `-ment`, `-al` or `-en` behind. |
| Number | A number, as figures or as a word: 3, three, third. |
| Project term | A word in the tables under "Project terms". |
| Name | A product, a language, a format, a standard, or a person. |

A command, a path, or an identifier goes in a code span, and a code span is not checked.
That is why a short form such as `spec` or `docs` needs no entry.
It names a command or a directory, and a sentence says "specification" or "documents".
A word inside double quotes is a mention, not a use, and is not checked.

A word from none of these sources is a question, not an error in itself.
Either a plainer word says the same, or the word is a term of the field.
A reader who looks up a term of the field reaches an official document.
Add such a term to a table below; write the plainer word otherwise.

## Project terms

### Terms of the field

| Area | Terms |
| --- | --- |
| Programs and builds | `app`, `backend`, `baseline`, `cache`, `compatible`, `compile`, `crate`, `directory`, `fetch`, `linkage`, `repository`, `runtime`, `standalone`, `submodule`, `toolchain`, `upstream` |
| Code | `alias`, `arithmetic`, `byte`, `callsite`, `dataflow`, `dispatch`, `exhaustive`, `fuse`, `hash`, `header`, `hoist`, `identifier`, `integer`, `invariant`, `iteration`, `literal`, `lookup`, `macro`, `mask`, `namespace`, `numeric`, `operand`, `precondition`, `recursion`, `temp`, `token`, `unsigned`, `wildcard` |
| Tests and tools | `benchmark`, `conformance`, `diff`, `harness`, `lint`, `linter`, `parse`, `parser`, `snapshot`, `suite`, `testsuite`, `verify` |
| Text and markup | `indentation`, `markup`, `placeholder`, `prefix`, `template` |

### Terms for writing

These terms describe a sentence, and they are allowed only where a document is about writing.
Those documents are `AGENTS.md`, this file, and decisions 98 and 99.
Anywhere else a use of one is likely a metaphor.

| Terms |
| --- |
| `abbreviation`, `bold`, `colon`, `dash`, `hyphen`, `idiom`, `imperative`, `intensifier`, `modifier`, `punctuation`, `semicolon`, `subordinate` |

### Names

A name keeps the spelling its owner gives it: Wasmtime the product, `wasmtime` the command.
A name that starts with a capital letter needs no entry, and neither does one in capitals.
The names below are the ones their owners write in lower case.

| Names |
| --- |
| `dewasm`, `textlint`, `wasm` |

## Excluded words

A word here is not written even when another source allows it.
The base list holds `gate`, `green`, `sweep`, and `wire`, since a metaphor is made of plain words.

A listed form matches as a whole word, in any letter case.
A match inside an `Except` phrase, a code span, or double quotes is a mention, not a use.
A check reports a use and never rewrites one.
The replacement depends on the sentence, so the `Write` column offers candidates, not a substitution.
List every form a word takes, because a check matches whole words.
Name an exception as a phrase, so a term of the field that shares the word stays usable.
The lists do not apply to this file.

### Metaphors

| Do not write | Write | Except |
| --- | --- | --- |
| `gate`, `gates`, `gated`, `gating` | check, required check, condition | — |
| `golden` | snapshot | — |
| `is green`, `are green`, `goes green`, `is red`, `are red`, `goes red` | passes, fails | — |
| `pin`, `pins`, `pinned`, `pinning` | fix, state | — |
| `sibling`, `siblings` | related | — |
| `sidecar`, `sidecars` | a file kept beside it | — |
| `sweep`, `sweeps`, `swept`, `sweeping` | full run, run over, update | `mark and sweep`, `mark-and-sweep` |
| `under the hood` | inside | — |
| `wire`, `wires`, `wired`, `wiring` | connect, implement, register, call | `wire format`, `on the wire` |

### One term per concept

| Do not write | Write | Except |
| --- | --- | --- |
| `tier`, `tiers`, `tiered`, `lane`, `lanes`, `speed token` | category | — |
| `invoke`, `invokes`, `invoked`, `invoking` | call for a function or a macro, run for a command | — |
| `opt in`, `opt-in`, `opt out`, `opt-out` | enable, optional: the words Cargo uses | — |
| `prose` | text | — |
| `connective`, `connectives` | linking word | — |

### Latin

| Do not write | Write | Except |
| --- | --- | --- |
| `status quo` | the current state | — |
| `verbatim` | unchanged | — |

### Intensifiers

An intensifier has no replacement: remove it, or state the fact it stood for.

| Do not write | Write | Except |
| --- | --- | --- |
| `byte for byte`, `byte-for-byte` | identical, with what was compared | — |
| `fully`, `simply` | nothing | — |

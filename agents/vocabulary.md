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

### Not derived forms

A word that only looks derived needs another source, as `siren` is not `sir` with `-en`.
The derivation rules skip a stem shorter than three letters and the words in the table below.

| Not a derived form |
| --- |
| `arity`, `binaryen`, `fatal`, `flattering`, `outlier`, `paren`, `parity`, `refactor`, `relay`, `resume`, `retention`, `retract`, `siren`, `unwasm`, `virtual`, `wasmer` |

A word from none of these sources is a question, not an error in itself.
Either a plainer word says the same, or the word is a term of the field.
A reader who looks up a term of the field reaches an official document.
Add such a term to a table below; write the plainer word otherwise.

## Project terms

### Terms of the field

| Area | Terms |
| --- | --- |
| Programs and builds | `app`, `archive`, `backend`, `baseline`, `browser`, `builtin`, `cache`, `compatible`, `compile`, `copyright`, `crate`, `default`, `dependency`, `directory`, `fetch`, `fork`, `frontend`, `gem`, `legacy`, `linkage`, `manifest`, `milestone`, `patch`, `registry`, `repository`, `reproducible`, `runtime`, `script`, `standalone`, `subagent`, `subcommand`, `submodule`, `toolchain`, `upstream`, `workflow`, `workspace` |
| Code | `accessor`, `alias`, `arithmetic`, `arity`, `autoboxing`, `byte`, `bytecode`, `callback`, `callee`, `callsite`, `constructor`, `dataflow`, `debug`, `defer`, `dispatch`, `elide`, `elision`, `epilogue`, `exhaustive`, `fallback`, `fuse`, `generic`, `hash`, `header`, `heap`, `hoist`, `identifier`, `incremental`, `inherit`, `initialize`, `initializer`, `inline`, `integer`, `interned`, `invariant`, `iteration`, `keyword`, `lambda`, `literal`, `lookup`, `macro`, `mask`, `mutex`, `namespace`, `numeric`, `offset`, `operand`, `optimization`, `optimize`, `override`, `peephole`, `pending`, `precedence`, `precondition`, `recursion`, `recursive`, `relay`, `sanitizer`, `scalar`, `selector`, `signature`, `spill`, `stub`, `subclass`, `thunk`, `token`, `trampoline`, `tuple`, `unsigned`, `virtual`, `wildcard` |
| Languages | `associative`, `associativity`, `bignum`, `fixnum`, `flonum`, `goroutine`, `nameref`, `splat`, `subscript`, `subshell`, `ternary`, `unary` |
| WebAssembly and numbers | `big-endian`, `bitwise`, `canonical`, `canonicalize`, `clamp`, `congruence`, `decimal`, `decode`, `dividend`, `exponent`, `gradual`, `hexadecimal`, `instantiate`, `instantiation`, `little-endian`, `modular`, `modulo`, `mutability`, `mutable`, `normalization`, `normalize`, `opcode`, `overflow`, `passive`, `payload`, `remainder`, `saturate`, `significand`, `softfloat`, `sticky`, `subnormal`, `tag`, `ulp`, `underflow`, `uninitialized`, `validate`, `validation`, `width` |
| Systems and WASI | `ambient`, `buffer`, `checksum`, `console`, `cursor`, `deterministic`, `flush`, `headless`, `interactive`, `monotonic`, `preopen`, `pseudo-terminal`, `sandbox`, `socket`, `subscription` |
| Data and records | `amalgamation`, `collation`, `compress`, `compression`, `decompress`, `invalidate`, `invalidation`, `query`, `rollback`, `schema`, `verdict` |
| Tests and tools | `benchmark`, `bug`, `calibrate`, `calibration`, `conformance`, `diff`, `fixture`, `glue`, `harness`, `lint`, `linter`, `lollipop`, `median`, `microbenchmark`, `microsecond`, `millisecond`, `nanosecond`, `oracle`, `parse`, `parser`, `sanity`, `shim`, `snapshot`, `suite`, `testsuite`, `verify`, `workload` |
| Graphics and games | `dot`, `emulator`, `framebuffer`, `palette`, `pixel`, `shareware`, `texture`, `tick` |
| Text and markup | `digit`, `font`, `indentation`, `interpolate`, `interpolation`, `kebab-case`, `markup`, `placeholder`, `prefix`, `slug`, `suffix`, `template` |

A term of the field is used only in the sense its document gives it.
Used for anything else, it is a metaphor: `saturate` names a clamping arithmetic, not a full thing.
A general term such as `bug` works better with its kind: a crash, a wrong result, a missed error.

### Terms of one context

A term below belongs to one program or one subject, and only the paths beside it use it.
Elsewhere the same word would be a term nobody defined for that text.

| Terms | Allowed in |
| --- | --- |
| `automap`, `demo`, `strafe`, `tic` | `examples/doom/`, `agents/decisions/50-doom-example-shape.md`, `agents/decisions/53-doom-frame-snapshot.md` |
| `alternate` (the terminal's alternate screen) | `examples/` |
| `balloon` (the speech balloon of `cowsay`) | `agents/decisions/95-cowsay-own-implementation.md` |

### Units

A unit below follows a number, as in `35 ms`; it is not a word of a sentence by itself.

| Units |
| --- |
| `ns`, `ms`, `s`, `kB`, `MB`, `GB`, `Hz`, `nanosecond`, `microsecond`, `millisecond` |

### Abbreviations

An abbreviation below is written in this one form, and no other.

| Abbreviation |
| --- |
| `e.g.`, `i.e.`, `etc.`, `vs.` |

### Terms with one meaning

A term below has one meaning here, and a sentence uses it for nothing else.
A short form is allowed as the name of one thing, never as a shorter way to write a longer word.

| Term | Means | For another meaning, write |
| --- | --- | --- |
| `temp` | The variable that holds one slot of the wasm value stack: the `Temp` of the IR. | temporary, as in "temporary file" |

### Terms for writing

These terms describe a sentence, and they are allowed only where a document is about writing.
Those documents are `AGENTS.md`, `agents/vocabulary.md`, and decisions 98 and 99:
`agents/decisions/98-sentence-length-bound.md` and `agents/decisions/99-allowed-vocabulary.md`.
Anywhere else a use of one is likely a metaphor.

| Terms |
| --- |
| `abbreviation`, `bold`, `colon`, `dash`, `hyphen`, `idiom`, `imperative`, `intensifier`, `modifier`, `punctuation`, `semicolon`, `subordinate` |

### Names

A name keeps the spelling its owner gives it: Wasmtime the product, `wasmtime` the command.
A name that starts with a capital letter needs no entry, and neither does one in capitals.
The names below are the ones their owners write in lower case.
A name that only starts a table cell is listed too, since the check reads it as a first word.

| Names |
| --- |
| `dewasm`, `Fedora`, `Fujinami`, `Hiroya`, `id`, `iNES`, `macOS`, `mruby`, `silicon`, `spaCy`, `textlint`, `wasm` |

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
| `tmp` | temp for the variable of the IR, temporary for a file; a path or a variable name goes in a code span | — |
| `temp file`, `temp files`, `temp dir`, `temp dirs`, `temp directory`, `temp directories`, `temp copy`, `temp copies`, `temp path`, `temp paths` | temporary file, temporary directory, temporary copy, temporary path | — |
| `invoke`, `invokes`, `invoked`, `invoking` | call for a function or a macro, run for a command | — |
| `opt in`, `opt-in`, `opt out`, `opt-out` | enable, optional: the words Cargo uses | — |
| `prose` | text | — |
| `connective`, `connectives` | linking word | — |

### Latin

| Do not write | Write | Except |
| --- | --- | --- |
| `status quo` | the current state | — |
| `verbatim` | unchanged | — |
| `cf`, `cf.` | see, refer to | — |
| `v.s.`, `vs`, `etc` | `vs.`, `etc.` | `vs.`, `etc.` |

### Intensifiers

An intensifier has no replacement: remove it, or state the fact it stood for.

| Do not write | Write | Except |
| --- | --- | --- |
| `byte for byte`, `byte-for-byte` | identical, with what was compared | — |
| `fully`, `simply` | nothing | — |

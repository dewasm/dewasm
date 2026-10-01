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
| `arity`, `binaryen`, `fatal`, `flattering`, `header`, `literal`, `outlier`, `paren`, `parity`, `refactor`, `relay`, `resume`, `retention`, `retract`, `siren`, `subscription`, `unwasm`, `virtual`, `wasmer` |

A word from none of these sources is a question, not an error in itself.
Either a plainer word says the same, or the word belongs in a table below.
A term of the field names a concept of computing.
A reader who looks it up reaches an official document.
A plain word outside the base list is listed when no plainer word of its length says the same.

The check reports a listed word that no text uses, and one that another source already allows.
Such an entry goes, so the tables hold only the words the text needs.

## Project terms

### Terms of the field

A term below names a concept of computing, and the text uses it only in that sense.
That holds where a dictionary also gives it a plain sense: `compile` never means to gather a list.

| Area | Terms |
| --- | --- |
| Programs and builds | `app`, `assemble`, `backend`, `backport`, `baseline`, `builtin`, `cache`, `checkout`, `codegen`, `compile`, `crate`, `deprecated`, `downstream`, `dylib`, `entrypoint`, `fork`, `frontend`, `gem`, `kernel`, `linkage`, `manifest`, `patch`, `pragma`, `prebuilt`, `registry`, `repository`, `runtime`, `standalone`, `subagent`, `subcommand`, `submodule`, `tarball`, `toolchain`, `upstream`, `vendor`, `workspace` |
| Code | `accessor`, `accumulator`, `alias`, `annotation`, `append`, `arity`, `autoboxing`, `blob`, `boolean`, `byte`, `bytecode`, `callback`, `callee`, `callsite`, `clobber`, `co-inductive`, `collector`, `collide`, `concatenate`, `constructor`, `dataflow`, `debug`, `defunctionalize`, `degenerate`, `dereference`, `devirtualize`, `disable`, `dispatch`, `duck-typed`, `dump`, `elide`, `emulate`, `epilogue`, `exhaustive`, `fallback`, `fallthrough`, `fuse`, `greedy`, `hash`, `header`, `heap`, `heuristic`, `hoist`, `hop`, `idempotent`, `immutable`, `inherit`, `initialize`, `inline`, `integer`, `intercept`, `interned`, `intrinsic`, `invariant`, `iteration`, `keyword`, `lambda`, `literal`, `lookup`, `macro`, `mask`, `materialize`, `memoize`, `mutex`, `namespace`, `negate`, `no-op`, `normalize`, `offset`, `operand`, `optimize`, `override`, `overwrite`, `padding`, `peephole`, `preamble`, `precedence`, `precondition`, `prelude`, `prologue`, `queue`, `recurse`, `recursive`, `relay`, `sanitizer`, `scalar`, `scratch`, `selector`, `serialize`, `signature`, `spill`, `struct`, `stub`, `subclass`, `substring`, `thunk`, `token`, `trampoline`, `truncate`, `tuple`, `upcast`, `variadic`, `virtual`, `wildcard`, `wraparound` |
| Languages | `associative`, `bignum`, `fixnum`, `flonum`, `goroutine`, `nameref`, `splat`, `subscript`, `subshell`, `ternary`, `unary` |
| WebAssembly and numbers | `ascending`, `big-endian`, `bitwise`, `canonical`, `canonicalize`, `clamp`, `congruence`, `decode`, `descending`, `dyadic`, `gradual`, `hexadecimal`, `instantiate`, `little-endian`, `malformed`, `modular`, `modulo`, `multiplicative`, `mutable`, `octal`, `opcode`, `overflow`, `overlong`, `passive`, `payload`, `pseudo-random`, `radicand`, `saturate`, `significand`, `softfloat`, `sticky`, `subnormal`, `tag`, `ulp`, `underflow`, `validate` |
| Systems and WASI | `buffer`, `checksum`, `cookie`, `cryptographic`, `dangling`, `deadlock`, `delimiter`, `durability`, `echo`, `endpoint`, `epoch`, `flush`, `hardware`, `headless`, `inode`, `metadata`, `multi-tenant`, `preopen`, `pseudo-terminal`, `pushback`, `sandbox`, `socket`, `spawn`, `stdio`, `subscription`, `sync`, `timeout` |
| Data and records | `affinity`, `amalgamation`, `amortize`, `categorical`, `collation`, `compress`, `congruential`, `decompress`, `generator`, `geometric`, `invalidate`, `percentile`, `quadratic`, `query`, `rollback`, `schema`, `superlinear`, `verdict` |
| Tests and tools | `benchmark`, `conformance`, `diff`, `fixture`, `glue`, `harness`, `lint`, `lollipop`, `microbenchmark`, `oracle`, `parse`, `shim`, `snapshot`, `suite`, `testsuite`, `transcript`, `workload` |
| Graphics and games | `alpha`, `dot`, `emulator`, `flicker`, `foreground`, `framebuffer`, `pixel`, `screenshot`, `shareware`, `tick`, `upscale` |
| Text and markup | `filename`, `glob`, `kebab-case`, `markup`, `newline`, `slug`, `whitespace` |

Used in another sense, a term is a metaphor: `saturate` names a clamping arithmetic, not a full thing.

### Plain words outside the base list

A word below means what a dictionary says, and it names no concept of computing.
It is listed because no plainer word of about the same length says the same.
It needs no document.
A general word such as `bug` works better with its kind: a crash, a wrong result, a missed error.

| Words |
| --- |
| `acceptance`, `align`, `alternating`, `ambient`, `ambiguous`, `animated`, `archive`, `arithmetic`, `backslash`, `batch`, `browser`, `bug`, `calibrate`, `capitalize`, `checklist`, `circularity`, `coexist`, `compatible`, `console`, `contiguous`, `copyright`, `counterpart`, `curated`, `cursor`, `decimal`, `default`, `defer`, `dependency`, `deterministic`, `digit`, `directory`, `dividend`, `divisor`, `duplicate`, `elapse`, `exponent`, `fake`, `falsifiable`, `fatal`, `fetch`, `font`, `generic`, `incremental`, `indentation`, `innermost`, `interactive`, `interleave`, `interpolate`, `invalid`, `keyboard`, `laptop`, `launder`, `legacy`, `lone`, `median`, `megabytes`, `microsecond`, `milestone`, `millisecond`, `monotonic`, `nanosecond`, `nonexistent`, `numeric`, `opaque`, `optimistic`, `outermost`, `outright`, `outward`, `palette`, `pending`, `persist`, `pessimistic`, `placeholder`, `plumbing`, `populate`, `prefix`, `proleptic`, `quit`, `quotient`, `remainder`, `reproducible`, `roadmap`, `sanity`, `script`, `separator`, `slideshow`, `snippet`, `sparse`, `suffix`, `template`, `texture`, `timestamp`, `trademark`, `verify`, `width`, `workflow` |

### Terms of one context

A term below belongs to one program or one subject, and only the paths beside it use it.
Elsewhere the same word would be a term nobody defined for that text.

| Terms | Allowed in |
| --- | --- |
| `automap`, `decal`, `demo`, `diminished`, `strafe`, `tic` | `examples/doom/`, `crates/dewasm-test-helper/src/doom.rs`, `agents/decisions/50-doom-example-shape.md`, `agents/decisions/53-doom-frame-snapshot.md` |
| `alternate` (the terminal's alternate screen) | `examples/` |
| `balloon` (the speech balloon of the cowsay program) | `agents/decisions/95-cowsay-own-implementation.md` |

### Units

A unit below follows a number, as in `35 ms`; it is not a word of a sentence by itself.

| Units |
| --- |
| `ns`, `ms`, `s`, `kB`, `MB`, `GB`, `Hz`, `nanosecond`, `microsecond`, `millisecond` |

### Abbreviations

An abbreviation below is written in this one form, and no other.

| Abbreviation |
| --- |
| `etc.`, `vs.` |

### Terms with one meaning

A term below has one meaning here, and a sentence uses it for nothing else.
A short form is allowed as the name of one thing, never as a shorter way to write a longer word.
A term with a space is allowed only as the whole phrase: "doc" alone is a short form to write out.

| Term | Means | For another meaning, write |
| --- | --- | --- |
| `temp` | The variable that holds one slot of the wasm value stack: the `Temp` of the IR. | temporary, as in "temporary file" |
| `doc comment` | Rust's comment that documents the item after it or around it: `///` and `//!`. | documentation, as in "the documentation of a crate" |

### Terms for writing

These terms describe a sentence, and they are allowed only where a document is about writing.
Those documents are `AGENTS.md`, `agents/vocabulary.md`, and decisions 98 and 99:
`agents/decisions/98-sentence-length-bound.md` and `agents/decisions/99-allowed-vocabulary.md`.
Anywhere else a use of one is likely a metaphor.

| Terms |
| --- |
| `abbreviation`, `bold`, `colon`, `dash`, `hyphen`, `idiom`, `imperative`, `intensifier`, `punctuation`, `semicolon`, `subordinate` |

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
| `e.g.`, `i.e.` | for example, that is | — |
| `v.s.`, `vs`, `etc` | `vs.`, `etc.` | `vs.`, `etc.` |

### Intensifiers

An intensifier has no replacement: remove it, or state the fact it stood for.

| Do not write | Write | Except |
| --- | --- | --- |
| `byte for byte`, `byte-for-byte` | identical, with what was compared | — |
| `fully`, `simply` | nothing | — |

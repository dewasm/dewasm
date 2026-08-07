# ADR-64: Record Distribution Size Beside Speed, in Raw Bytes

**Status:** Accepted (2026-08-06). Landed: `cargo xtask size` (`crates/xtask/src/size/`), the record under `benchmarks/results/` as `<timestamp>Z-size.json`, and the generated `docs/sizes/results.md` with its figures beside a hand-written `docs/sizes/README.md`. Not covered: any CI enforcement — like the benchmark record ([ADR-57](57-benchmark-harness.md)) this is a dated measurement, not a compared snapshot.

## Context

[ADR-57](57-benchmark-harness.md) made speed measurable, and speed is the axis on which dewasm loses: converted source is orders of magnitude slower than an AOT runtime. Size is the axis on which it can win, and it was unmeasured — so the distribution argument ("ship source instead of a binary plus a runtime") was being made in prose with no numbers behind it.

The claim is falsifiable and worth checking, because the answer is not obviously favourable. Converted source is much larger than the wasm it came from — a factor of 3 to 30 across the backends. What it replaces is not the wasm alone but the wasm *plus a runtime that can execute it*, and those runtimes differ by more than two orders of magnitude among themselves: on the host this landed on, `wasm3` is 175 kB and `wasmer` is 53.4 MB. Whether source undercuts binary-plus-runtime therefore depends on the app, on the backend, and on which runtime the comparison assumes.

The measurement also has to survive the size work that follows it. Every change that shrinks generated code needs somewhere to show its effect, and a number quoted by hand in a README rots the first time codegen changes.

## Decision

**A size record, `cargo xtask size`, built as the sibling of `cargo xtask bench`** — same shape, same conventions, deliberately: a fixed corpus, a dated JSON record, a generated document with SVG figures, a `--render` flag that rebuilds the document from a stored record without measuring, and skipped-with-reason for anything missing ([ADR-15](15-tests-fail-not-skip.md)). It shares the benchmark's drawing code, so a size figure and a speed figure are the same picture in different units.

**The records live in `benchmarks/results/`, beside the speed records** — the same dated filename with a `-size` suffix naming the kind. A maintainer decision, overriding the initial `docs/sizes/records/`: measurement records get one home, not one per kind, because two conventions for the same thing means two places to look and two rules to remember. The suffix is enough to tell the kinds apart, and `--render` pointed at the other kind fails to deserialize, which is the only check that matters.

**The generated document is `docs/sizes/results.md`, with a hand-written `docs/sizes/README.md` beside it**, exactly as `docs/benchmarks/` is laid out — also a maintainer decision, overriding an initial single generated `README.md`. The generated file carries numbers: the environment, the tables, the figures, and what was not measured. Everything a reader needs in order to interpret those numbers — how to run the command, what a runtime's size includes, the caveats — is prose, prose is written by a person, and a person cannot edit a generated file. That split also keeps the explanations out of every regeneration's diff.

**Raw bytes, never compressed.** What a release artifact weighs is the number a person distributing it pays. Compression is also not neutral between the two sides being compared: source compresses far better than a binary, so a gzip column would flatten exactly the differences this record exists to track, and every later size improvement would show up smaller than it is.

**The corpus is fixed at four apps** — cowsay, sqlite3-shell, qjs, ruby — spanning two orders of magnitude of wasm size, rather than "whatever is in the cache". A record whose contents depend on which apps happened to be built is not comparable with the next one.

**A runtime's size is its executable plus the shared libraries that executable actually loads.** Neither half of that rule is optional. Homebrew's `wasmedge` command-line tool is 100 kB of front end over a 2.4 MB `libwasmedge`, so the executable alone understates it twentyfold; the `lib/` directory beside `wasmtime` holds a 55 MB embedding SDK that its statically linked CLI never opens, so counting the directory overstates it twofold. What is counted is what the executable names: a candidate library beside it is included only when its filename appears in the executable's own bytes, which is where the dynamic linker's dependency list lives. The record stores every counted file with its path and size, so the accounting can be checked against the host rather than trusted.

**Two fairness caveats are stated, not left to the reader.** Converted source presumes the target language's interpreter is already installed — the premise of shipping to that language's users, but a presumption. And a runtime binary is one platform's delivery while source is every platform's. Both are written down in `docs/sizes/README.md`, with the rest of the reading instructions.

## Rejected alternatives

**Record gzip sizes too, or instead.** Rejected by the maintainer. Raw bytes are the honest distribution footprint, and compression compresses the two sides at different rates, which turns a size comparison into a comparison of redundancy.

**Keep the records under `docs/sizes/records/`, next to the document they render into.** What the implementation did first, and rejected by the maintainer: it splits record storage by kind, so the project would carry two conventions for one thing — a speed record in `benchmarks/results/`, a size record somewhere else — and a reader looking for "the measurements" would have to know which kind they wanted before they could find either. The document's proximity to its record is worth less than one home for all of them.

**Derive sentences in the generated document** — "smallest converted source: X, N times the binary plus a runtime". Computed from the record, so never stale, and still rejected by the maintainer: a generated file that argues is a file people want to edit, the comparison it picks is one of many a reader might want, and the tables already carry every number needed to make it. The generated file states; `README.md` explains.

**Measure a runtime as its executable only.** The simple rule, and it reports wasmedge as the smallest runtime measured here by a factor of twenty, which is an artifact of how one distribution packages it.

**Extend `cargo xtask bench` with size columns.** One command, one record. But a size measurement needs no runtime installed, no calibration and no correctness oracle, and takes minutes rather than tens of minutes; folding it in would make every size number wait for a benchmark run, and a filtered benchmark run would silently produce a partial size record.

**A test that fails when a size regresses.** Attractive, and premature: the thresholds would be guesses, and the number moves with the host's wasm binaries as well as with codegen. The record is what a size PR shows its effect in; enforcement can come later if the numbers turn out to be stable enough to bound.

## Consequences

- The distribution claim now has numbers, including where it fails. Against `wasm3` converted source loses on every app in the corpus; against `wasmer` or `wasmtime` the smaller backends win on the smaller apps, and on `ruby.wasm` the closest backend lands within a few percent of the binary-plus-runtime pairing.
- A full run takes minutes (it converts four apps with six backends, holding several hundred MB of generated source in memory for the largest pair) and is deliberately outside `cargo test`.
- The record is host-specific and dated: the runtime sizes are whatever that host has installed. Two records from different hosts compare the source columns, not the runtime ones.
- The benchmark's chart module now draws with the unit left open (`Units`), and its `Theme`, `Family` and `Row` are shared. A change to either record's look changes both, which is the intent.

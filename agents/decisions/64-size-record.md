# Decision 64: Record Distribution Size Beside Speed, in Raw Bytes

**Status:** Accepted (2026-08-06).
Landed:

- `cargo xtask size` (`crates/xtask-records/src/size/`);
- the record under `benchmarks/results/` as `<timestamp>Z-size.json`;
- the generated `docs/sizes/results.md` with its figures, beside a hand-written `docs/sizes/README.md`.

Not covered: any CI enforcement.
Like the benchmark record ([decision 57](57-benchmark-harness.md)), this is a dated measurement.
It is not a compared snapshot.

## Context

[Decision 57](57-benchmark-harness.md) made speed measurable, and speed is the axis on which dewasm loses.
Converted source is orders of magnitude slower than an AOT runtime.
Size is the axis on which it can win, and it was unmeasured.
So the distribution argument was being made in text with no numbers behind it.
The argument is "ship source instead of a binary plus a runtime".

The claim is falsifiable and worth checking, because the answer is not obviously favourable.
Converted source is much larger than the wasm it came from, a factor of 3 to 30 across the backends.
What it replaces is not the wasm alone but the wasm *plus a runtime that can execute it*.
Those runtimes differ by more than two orders of magnitude among themselves.
On the host this landed on, `wasm3` is 175 kB and `wasmer` is 53.4 MB.
Whether source is smaller than binary-plus-runtime therefore depends on the app and on the backend.
It also depends on which runtime the comparison assumes.

The measurement also has to survive the size work that follows it.
Every change that reduces generated code needs somewhere to show its effect.
A number quoted by hand in a README goes out of date the first time code generation changes.

## Decision

**A size record, `cargo xtask size`, built on the model of `cargo xtask bench`.**
It deliberately has the same shape and the same conventions:

- a fixed corpus;
- a dated JSON record;
- a generated document with SVG figures;
- a `--render` flag that rebuilds the document from a stored record without measuring;
- skipped-with-reason for anything missing ([decision 15](15-tests-fail-not-skip.md)).

It shares the benchmark's drawing code.
So a size figure and a speed figure are the same picture in different units.

**The records live in `benchmarks/results/`, beside the speed records.**
They use the same dated file name with a `-size` suffix naming the kind.
This is a maintainer decision, overriding the initial `docs/sizes/records/`.
Measurement records get one home, not one per kind.
The reason: two conventions for the same thing means two places to look and two rules to remember.
The suffix is enough to tell the kinds apart.
`--render` pointed at the other kind fails to parse, which is the only check that matters.

**The generated document is `docs/sizes/results.md`, with a hand-written `docs/sizes/README.md`.**
This is exactly how `docs/benchmarks/` is laid out.
It is also a maintainer decision, overriding an initial single generated `README.md`.
The generated file carries numbers.
Those are the environment, the tables, the figures, and what was not measured.
Everything a reader needs in order to interpret those numbers is text.
That is how to run the command, what a runtime's size includes, and the limits.
That text is written by a person, and a person cannot edit a generated file.
That split also keeps the explanations out of every regeneration's diff.

**Raw bytes, never compressed.**
What a release artifact weighs is the number a person distributing it pays.
Compression is also not neutral between the two sides being compared.
That is because source compresses far better than a binary.
So a `gzip` column would flatten exactly the differences this record exists to track.
Every later size improvement would also show up smaller than it is.

**The corpus is fixed at four apps** (`cowsay`, `sqlite3-shell`, `qjs`, `ruby`).
They span two orders of magnitude of wasm size; the corpus is not "whatever is in the cache".
A record whose contents depend on which apps happened to be built is not comparable with the next.

**A runtime's size is its executable plus the shared libraries that executable actually loads.**
Neither half of that rule is optional.
Homebrew's `wasmedge` command-line tool is 100 kB of front end over a 2.4 MB `libwasmedge`.
So the executable alone counts about a twentieth of it.
The `lib/` directory beside `wasmtime` holds a 55 MB embedding SDK.
The statically linked `wasmtime` CLI never opens it.
So counting the directory gives twice its size.
What is counted is what the executable names.
A candidate library beside it is included only when its file name appears in the executable's bytes.
Those bytes are where the dynamic linker's dependency list lives.
The record stores every counted file with its path and size.
So the accounting can be checked against the host rather than trusted.

**Two fairness limits are stated, not left to the reader.**
Converted source presumes the target language's interpreter is already installed.
That is the premise of shipping to that language's users, but still an assumption.
And a runtime binary is one platform's delivery while source is every platform's.
Both are written down in `docs/sizes/README.md`, with the rest of the reading instructions.

## Rejected alternatives

**Record `gzip` sizes too, or instead.**
Rejected by the maintainer.
Raw bytes are the honest distribution size.
Compression compresses the two sides at different rates.
That turns a size comparison into a comparison of how much each side repeats itself.

**Keep the records under `docs/sizes/records/`, next to the document they render into.**
The implementation did this first, and the maintainer rejected it.
It splits record storage by kind.
So the project would carry two conventions for one thing.
A speed record would be in `benchmarks/results/`, a size record somewhere else.
So a reader looking for "the measurements" would have to know which kind they wanted to find either.
The document being next to its record is worth less than one home for all of them.

**Derive sentences in the generated document.**
An example: "smallest converted source: X, N times the binary plus a runtime".
It is computed from the record, so it is never out of date.
The maintainer still rejected it, for three reasons:

- a generated file that argues is a file people want to edit;
- the comparison it picks is one of many a reader might want;
- the tables already carry every number needed to make it.

The generated file states; `README.md` explains.

**Measure a runtime as its executable only.**
The simple rule.
It reports WasmEdge as the smallest runtime measured here by a factor of twenty.
That is an artifact of how one distribution packages it.

**Extend `cargo xtask bench` with size columns.**
One command, one record.
But a size measurement needs no runtime installed, no calibration and no correctness oracle.
It takes minutes rather than tens of minutes.
Folding it in would make every size number wait for a benchmark run.
A filtered benchmark run would also silently produce a partial size record.

**A test that fails when a size gets worse.**
Attractive, and too early: the thresholds would be guesses.
That is also because the number moves with the host's wasm binaries as well as with code generation.
The record is what a size PR shows its effect in.
Enforcement can come later if the numbers turn out to be stable enough to bound.

## Consequences

- The distribution claim now has numbers, including where it fails.
  Against `wasm3`, converted source loses on every app in the corpus.
  Against `wasmer` or `wasmtime`, the smaller backends win on the smaller apps.
  On `ruby.wasm`, the closest backend lands within a few percent of the binary-plus-runtime pairing.
- A full run takes minutes and is deliberately outside `cargo test`.
  It converts four apps with six backends.
  It holds several hundred MB of generated source in memory for the largest pair.
- The record is host-specific and dated: the runtime sizes are whatever that host has installed.
  Two records from different hosts compare the source columns, not the runtime ones.
- The benchmark's chart module now draws with the unit left open (`Units`).
  Its `Theme`, `Family` and `Row` are shared.
  A change to either record's look changes both, which is intended.

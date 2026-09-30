# Decision 68: Measurement Records at the Top Level, Written and Rendered by Paired Commands

Status: **Accepted, 2026-08-12.**
Landed:

- the stored records moved from `benchmarks/results/` to `records/`;
- each carries a kind suffix (`-speed.json`, `-size.json`);
- the two commands split into four: `record-speed`, `record-size`, `render-speed`, `render-size`;
- the commands live in `crates/xtask/src/bench/mod.rs` and `crates/xtask/src/size/mod.rs`;
- `records/README.md` carries one line per record file;
- the command that writes a record adds that line as a placeholder.

This revises the storage-location clause of [decision 64](64-size-record.md).
Everything else that decision fixed stands unchanged.
That is raw bytes, the fixed corpus, and what a runtime's size counts.

## Context

`benchmarks/` is the workload definitions:

- the hand-written `.wat`;
- the C microbenchmark sources with their build scripts;
- the drivers that run a module under `pywasm` and `wardite`;
- the build cache, ignored by Git.

[Decision 64](64-size-record.md) put the size record in `benchmarks/results/`.
It did so that every measurement record has one home rather than one per kind.
The home was the right call and its location was not.
A size record measures no benchmark.
The path claimed a containment that does not hold.
So the command and the documents around it had to keep explaining it.
They explained why a size record lives under the benchmark suite.

The two kinds were also named and driven differently.
The difference only tracked which one came first.
A size record was `<timestamp>Z-size.json` and a speed record was the unmarked `<timestamp>Z.json`.
So the older kind read as the default and the newer one as a variant.
Each kind had one command that measured and then rendered.
A `--render FILE` flag did only the second half.
Rendering, the cheap and repeatable half, was reachable only as an option of the expensive one.
The useful case meant typing the path of a record again, time of the run included.
That case is rendering the newest record after editing the renderer's text.

A record carries no account of itself either.
The JSON states the host and the time of the run, which is what a measurement can know.
It cannot state why it was taken.
Three of the stored records were taken within two and a half hours of one day.
They differ only by the code between them.
That code is the generated-source size reduction, then the Ruby parenthesis elision.
Nothing on disk said so.
That context existed only in the commit that added each file.
It was recoverable only by someone who thinks to run `git log --diff-filter=A` on it.
A record whose reason is that hard to reach reads as noise when someone next considers removing it.

## Decision

**Measurement records live in a top-level `records/`**, one directory for every kind.
It is named for what the files are rather than for the command that produced them.
The file name names the kind on both sides: `<timestamp>Z-speed.json` and `<timestamp>Z-size.json`.
The rendered documents do not move: `docs/benchmarks/results.md` and `docs/sizes/results.md`.
They are for a human reading the numbers, and stay where a human looks for them.

**Measuring and rendering are separate commands.**
They are `record-speed` / `record-size` and `render-speed` / `render-size`.
A record command writes its record and renders nothing.
It closes with the line that names the render command for its kind.
A render command reads one record and writes the document with its figures.
A render command with no argument takes the newest record of its own kind.
With ISO times in the names, the newest record sorts last by name.
So the ordinary case needs no path at all.
That case is rendering what was just measured, or re-rendering after editing the renderer.
The suffix is what tells the kinds apart, so it is checked rather than trusted to a parse error.
A `-size.json` handed to `render-speed` is refused by name.
A file name with neither suffix is an error wherever a record's kind is read.

**`records/README.md` documents every record file**, one line per file saying why it was taken.
The occasion is the one thing a measurement cannot supply.
So a record command adds a line for the file it writes, with the occasion left as a TODO.
The line goes under that kind's heading, and is added only when the file has no line yet.
A run therefore leaves an unexplained record visible in the diff of the commit that would add it.
There the person committing it still knows the answer.
A unit test asserts that every stored record has a line and a kind suffix.
The test is in `crates/xtask/src/bench/mod.rs`.

**Deciding criterion:** *name a thing for what it is rather than for the command that made it.*
*Give the two halves of a workflow equal standing when one of them is cheap.*
*An expensive step must not be the only door to a cheap one.*

## Rejected alternatives

- **Keep the records under `benchmarks/results/`.**
  Free, and it is the arrangement that made the size command explain its own path.
  It did so in three separate documentation comments.
  It would also put the index of measurements inside the workload directory.
  A reader looking for measurements has no reason to go there.

- **Split into `records/bench/` and `records/size/`.**
  This reintroduces per-kind storage, which decision 64 rejected for the reason that still holds.
  A reader looking for "the measurements" would first have to know which kind they wanted.
  The suffix already names the kind, and the directory holds seven files.

- **Leave the speed records unsuffixed.**
  The case for it: the suffix only has to separate the newer kind from the older one.
  True as a parsing matter and false as a naming one.
  It makes "speed" the meaning of a missing suffix, which nothing in the file name says.
  Every reader who has to ask what an unsuffixed record is pays for the two characters saved.

- **Keep one command per kind with `--render FILE`.**
  It works, and it puts the repeatable half of the job behind a flag on the half that takes an hour.
  It always needs a path to type.
  Splitting the commands is also what lets a record command say plainly that it rendered nothing.

- **Leave `records/README.md` entirely hand-written.**
  The failure mode is not a wrong line, it is a missing one.
  Records accumulate silently, each cheap to add and impossible to explain later.
  Adding the placeholder costs one function and converts that silence into a line in the diff.

- **Fail the run when the occasion is still a TODO.**
  The occasion is unknown at the moment the record is written.
  So the only run that could pass is one whose record was already documented before it existed.

## Consequences

- Positive: `records/` is discoverable from the repository root.
  Its file names say what each file is.
  The commands write to a path neither has to justify.
- Positive: a wording change in either generated document costs one command that measures nothing.
- Positive: every record now says why it exists.
  So the older ones have a stated reason to stay, not only no reason to go.
- Negative: links to `benchmarks/results/...` from outside the repository do not redirect.
  The three renamed speed records break any reference to their old file names.
  The historical mentions inside this directory keep the old paths deliberately.
  They record what was true then.
- Carry-over: the test checks that a line exists, not that its occasion was filled in.
  A TODO reaching main is a review miss, not a mechanical one.

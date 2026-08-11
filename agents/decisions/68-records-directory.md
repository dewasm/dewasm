# Decision 68: Measurement Records at the Top Level, Written and Rendered by Symmetric Commands

Status: **Accepted, 2026-08-12.**
Landed: the stored records moved from `benchmarks/results/` to `records/` with a kind suffix each (`-speed.json`, `-size.json`), the two commands split into four (`record-speed`, `record-size`, `render-speed`, `render-size` in `crates/xtask/src/bench/mod.rs` and `crates/xtask/src/size/mod.rs`), and `records/README.md` carrying one line per record file, appended as a placeholder by the command that writes the record.
This revises the storage-location clause of [decision 64](64-size-record.md); everything else that decision fixed (raw bytes, the fixed corpus, what a runtime's size counts) stands unchanged.

## Context

`benchmarks/` is the workload definitions: the hand-written `.wat` and the C microbenchmark sources with their build scripts, the drivers that run a module under pywasm and wardite, and the gitignored build cache.
[Decision 64](64-size-record.md) put the size record in `benchmarks/results/` so that every measurement record has one home rather than one per kind.
The home was the right call and its location was not: a size record measures no benchmark, and the path claimed a containment that does not hold, so both the command and the documents around it had to keep explaining why a size record lives under the benchmark suite.

The two kinds were also named and driven asymmetrically, in a way that only tracked which one came first.
A size record was `<timestamp>Z-size.json` and a speed record was the unmarked `<timestamp>Z.json`, so the older kind read as the default and the newer one as a variant.
Each kind had one command that measured and then rendered, with a `--render FILE` flag to do only the second half: rendering, the cheap and repeatable half, was reachable only as an option of the expensive one, and the useful case (render the newest record after editing the renderer's prose) meant retyping a timestamped path.

A record carries no account of itself either.
The JSON states the host and the timestamp, which is what a measurement can know; it cannot state why it was taken.
Three of the stored records were taken within two and a half hours of one day and differ only by the code between them (the generated-source shrink, then the Ruby parenthesis elision), and nothing on disk said so.
That context existed only in the commit that added each file, recoverable only by someone who thinks to run `git log --diff-filter=A` on it, and a record whose reason is that hard to reach reads as noise the next time someone considers deleting it.

## Decision

**Measurement records live in a top-level `records/`**, one directory for every kind, named for what the files are rather than for the command that produced them.
The filename names the kind on both sides: `<timestamp>Z-speed.json` and `<timestamp>Z-size.json`.
The rendered documents do not move: `docs/benchmarks/results.md` and `docs/sizes/results.md` are for a human reading the numbers, and stay where a human looks for them.

**Measuring and rendering are separate commands**, `record-speed` / `record-size` and `render-speed` / `render-size`.
A record command writes its record and renders nothing, closing with the line that names the render command for its kind; a render command reads one record and writes the document with its figures.
A render command with no argument takes the newest record of its own kind, which the ISO timestamps make a lexicographic maximum, so the ordinary case (render what was just measured, or re-render after editing the renderer) needs no path at all.
The suffix is what tells the kinds apart, so it is checked rather than trusted to a parse error: a `-size.json` handed to `render-speed` is refused by name, and a filename with neither suffix is an error wherever a record's kind is read.

**`records/README.md` documents every record file**, one line per file saying why it was taken.
The occasion is the one thing a measurement cannot supply, so a record command appends a line for the file it writes with the occasion left as a TODO, under that kind's heading, and appends only when the file has no line yet.
A run therefore leaves an unexplained record visible in the diff of the commit that would add it, where the person committing it still knows the answer.
A unit test asserts that every stored record has a line and a kind suffix (`crates/xtask/src/bench/mod.rs`).

**Discriminating criterion:** *name a thing for what it is rather than for the command that made it, and give the two halves of a workflow equal standing when one of them is cheap: an expensive step must not be the only door to a cheap one.*

## Rejected alternatives

- **Keep the records under `benchmarks/results/`.**
  Free, and it is the arrangement that made the size command explain its own path in three separate doc comments.
  It would also put the index of measurements inside the workload directory, where a reader looking for measurements has no reason to go.

- **Split into `records/bench/` and `records/size/`.**
  This reintroduces per-kind storage, which decision 64 rejected for the reason that still holds: a reader looking for "the measurements" would have to know which kind they wanted before they could find either.
  The suffix already names the kind, and the directory holds seven files.

- **Leave the speed records unsuffixed, since the suffix only has to separate the newer kind from the older one.**
  True as a parsing matter and false as a naming one: it makes "speed" the meaning of an absent suffix, which nothing in the filename says, and every reader who has to ask what an unsuffixed record is pays for the two characters saved.

- **Keep one command per kind with `--render FILE`.**
  It works, and it puts the repeatable half of the job behind a flag on the half that takes an hour, always with a path to type.
  Splitting the commands is also what lets a record command say plainly that it rendered nothing.

- **Leave `records/README.md` entirely hand-written.**
  The failure mode is not a wrong line, it is a missing one: records accumulate silently, each cheap to add and impossible to explain later.
  Appending the placeholder costs one function and converts that silence into a line in the diff.

- **Fail the run when the occasion is still a TODO.**
  The occasion is unknown at the moment the record is written, so the only run that could pass is one whose record was already documented before it existed.

## Consequences

- Positive: `records/` is discoverable from the repository root, its filenames say what each file is, and the commands write to a path neither has to justify.
- Positive: a wording change in either generated document costs one command that measures nothing.
- Positive: every record now says why it exists, which is what makes keeping the older ones defensible instead of merely tolerated.
- Negative: links to `benchmarks/results/...` from outside the repository do not redirect, and the three renamed speed records break any reference to their old filenames.
  The historical mentions inside this directory keep the old paths deliberately, because they record what was true then.
- Carry-over: the test enforces that a line exists, not that its occasion was filled in.
  A TODO reaching main is a review miss, not a mechanical one.

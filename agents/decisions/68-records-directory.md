# Decision 68: Measurement Records at the Top Level, Each Documented in `records/README.md`

Status: **Accepted, 2026-08-12.**
Landed: the five stored records moved from `benchmarks/results/` to `records/`, `cargo xtask bench` and `cargo xtask size` writing and rendering from there, and `records/README.md` carrying one section per record file, appended as a placeholder by the command that writes the record.
This revises the storage-location clause of [decision 64](64-size-record.md); everything else that decision fixed (raw bytes, the fixed corpus, what a runtime's size counts) stands unchanged.

## Context

`benchmarks/` is the workload definitions: the hand-written `.wat` and the C microbenchmark sources with their build scripts, the drivers that run a module under pywasm and wardite, and the gitignored build cache.
[Decision 64](64-size-record.md) put the size record in `benchmarks/results/` so that every measurement record has one home rather than one per kind.
The home was the right call and its location was not: a size record measures no benchmark, and the path claimed a containment that does not hold, so both the command and the documents around it had to keep explaining why a size record lives under the benchmark suite.

A record also carries no account of itself.
The JSON states the host and the timestamp, which is what a measurement can know; it cannot state why it was taken.
Three of the five stored records were taken within two and a half hours of one day and differ only by the code between them (the generated-source shrink, then the Ruby parenthesis elision), and nothing on disk said so.
That context existed only in the commit that added each file, recoverable only by someone who thinks to run `git log --diff-filter=A` on it, and a record whose reason is that hard to reach reads as noise the next time someone considers deleting it.

## Decision

**Measurement records live in a top-level `records/`**, one directory for every kind, named for what the files are rather than for the command that produced them.
The filename continues to name the kind: `<timestamp>Z.json` for speed, `<timestamp>Z-size.json` for size.
The rendered documents do not move: `docs/benchmarks/results.md` and `docs/sizes/results.md` are for a human reading the numbers, and stay where a human looks for them.

**`records/README.md` documents every record file**, one section per file with the same three keys: when it was taken, on what host, and on what occasion.
The occasion is the only one of the three that a measurement cannot supply, so `cargo xtask bench` and `cargo xtask size` append a section for the record they write with the occasion left as a TODO, and append only when the file has no section yet.
A run therefore leaves an unexplained record visible in the diff of the commit that would add it, where the person committing it still knows the answer.
A unit test asserts that every stored record has a section (`crates/xtask/src/bench/mod.rs`).

**Discriminating criterion:** *a directory is named for what its contents are, not for which command happens to write them; and where a stored artifact needs context the artifact cannot carry, the command that writes it opens the place to write that context, rather than leaving it to be reconstructed from history.*

## Rejected alternatives

- **Keep the records under `benchmarks/results/`.**
  Free, and it is the arrangement that made the size command explain its own path in three separate doc comments.
  It would also put the index of measurements inside the workload directory, where a reader looking for measurements has no reason to go.

- **Split into `records/bench/` and `records/size/`.**
  This reintroduces per-kind storage, which decision 64 rejected for the reason that still holds: a reader looking for "the measurements" would have to know which kind they wanted before they could find either.
  The suffix already names the kind, `--render` on the wrong kind fails to deserialize, and the directory holds five files.

- **Leave `records/README.md` entirely hand-written.**
  The failure mode is not a wrong section, it is a missing one: records accumulate silently, each cheap to add and impossible to explain later.
  Appending the placeholder costs one function and converts that silence into a line in the diff.

- **Fail the run when the occasion is still a TODO.**
  The occasion is unknown at the moment the record is written, so the only run that could pass is one whose record was already documented before it existed.

## Consequences

- Positive: `records/` is discoverable from the repository root, and the two commands write to a path neither has to justify.
- Positive: every record now says why it exists, which is what makes keeping the older ones defensible instead of merely tolerated.
- Negative: links to `benchmarks/results/...` from outside the repository do not redirect.
  The historical mentions inside this directory keep the old path deliberately, because they record what was true then.
- Carry-over: the test enforces that a section exists, not that its occasion was filled in.
  A TODO reaching main is a review miss, not a mechanical one.

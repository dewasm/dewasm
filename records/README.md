# Measurement records

Every measurement dewasm keeps: the dated JSON records `cargo xtask bench` (speed) and `cargo xtask size` (distribution size) write, one file per run.
The generated documents that present them, [docs/benchmarks/results.md](../docs/benchmarks/results.md) and [docs/sizes/results.md](../docs/sizes/results.md), are rendered from the most recent record of each kind; the older ones are kept as measurement history and can be re-rendered with `--render`.

Every record file has a section here saying when it was taken, on what host, and on what occasion.
A run appends its own section with the occasion left as `TODO`, because that is the one thing the measurement does not know about itself: fill it in when committing the record.

## 2026-08-02T02-41-22Z.json

- **Taken**: 2026-08-02.
- **Host**: macOS 26.5.2 (Darwin 25.5.0), Apple M1 Pro, aarch64.
- **Occasion**: the first published speed record, taken with the benchmark suite that introduced it (#106), covering the full 18-runner by 11-workload matrix with no correctness failure.

## 2026-08-03T16-23-15Z.json

- **Taken**: 2026-08-03.
- **Host**: macOS 26.5.2 (Darwin 25.5.0), Apple M1 Pro, aarch64.
- **Occasion**: a re-run of the same matrix on merged main to measure value-addressed branches and the halved generated source (#113, "Refresh the published benchmark record after #110").
  The wasmtime baselines match the 2026-08-02 record to within 2%, which is what makes the two comparable.

## 2026-08-06T02-31-05Z-size.json

- **Taken**: 2026-08-06.
- **Host**: macOS 26.5.2 (Darwin 25.5.0), Apple M1 Pro, aarch64.
- **Occasion**: the first size record, taken with the command that introduced it (#166 for #161), so the size-reduction work queued behind it had a starting point to be measured against.

## 2026-08-06T03-52-05Z-size.json

- **Taken**: 2026-08-06.
- **Host**: macOS 26.5.2 (Darwin 25.5.0), Apple M1 Pro, aarch64.
- **Occasion**: taken for the generated-source shrink (#167, "Shrink generated source: tabs, chained init, boolean fusion, include Rt, @m"), which is what the drop against the previous record measures; every backend but Go moved.

## 2026-08-06T04-31-17Z-size.json

- **Taken**: 2026-08-06.
- **Host**: macOS 26.5.2 (Darwin 25.5.0), Apple M1 Pro, aarch64.
- **Occasion**: taken for the parenthesis elision in the Ruby backend (#168 for #163); only the Ruby figures move against the previous record.

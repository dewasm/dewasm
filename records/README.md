# Measurement records

Every measurement dewasm keeps: the dated JSON records `cargo xtask record-speed` and `cargo xtask record-size` write, one file per run.
A record's suffix names its kind, `-speed.json` or `-size.json`.
The generated documents, [docs/benchmarks/results.md](../docs/benchmarks/results.md) and [docs/sizes/results.md](../docs/sizes/results.md), are rendered from the newest record of each kind by `cargo xtask render-speed` and `cargo xtask render-size`; an older record is measurement history, re-renderable by naming it (`cargo xtask render-speed records/<file>-speed.json`).

Every record file has one line here saying why it was taken.
A run appends its line with a `TODO`; fill it in when committing the record.

## Speed records

- `2026-08-02T02-41-22Z-speed.json`: the first record, with the suite that introduced it (#106).
- `2026-08-03T16-23-15Z-speed.json`: value-addressed branches and the halved generated source (#113).
- `2026-08-11T18-25-24Z-speed.json`: re-baseline after #176, #167/#168, and #195's unit-comment rewrites.

## Size records

- `2026-08-06T02-31-05Z-size.json`: the first size record (#166).
- `2026-08-06T03-52-05Z-size.json`: the generated-source shrink (#167).
- `2026-08-06T04-31-17Z-size.json`: Ruby parenthesis elision (#168).
- `2026-08-11T18-27-09Z-size.json`: re-baseline beside the same day's speed record.

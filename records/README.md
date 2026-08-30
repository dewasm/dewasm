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
- `2026-08-16T09-22-02Z-speed.json`: re-baseline after the issue #164 mask elision and memory-unit rework (#224 to #245).
- `2026-08-21T02-38-15Z-speed.json`: TODO: describe the occasion.
- `2026-08-21T03-50-04Z-speed.json`: TODO: describe the occasion.
- `2026-08-22T06-26-23Z-speed.json`: the first record with the shortened suite and the eight new wat cases (#266); measured 29 minutes wall.
- `2026-08-22T08-27-05Z-speed.json`: re-baseline after the f32 rounding change (#268).
- `2026-08-24T22-56-29Z-speed.json`: TODO: describe the occasion.
- `2026-08-24T23-27-21Z-speed.json`: TODO: describe the occasion.
- `2026-08-30T05-11-20Z-speed.json`: the first record with the converted-wasm3 runners (#279) on the wasm3 v0.9.0 pin (#278).

## Size records

- `2026-08-06T02-31-05Z-size.json`: the first size record (#166).
- `2026-08-06T03-52-05Z-size.json`: the generated-source shrink (#167).
- `2026-08-06T04-31-17Z-size.json`: Ruby parenthesis elision (#168).
- `2026-08-11T18-27-09Z-size.json`: re-baseline beside the same day's speed record.
- `2026-08-16T09-23-36Z-size.json`: the issue #164 reductions measured: every Ruby and Python output shrinks (sqlite3-shell 7.94 to 7.24 MB).
- `2026-08-21T04-00-29Z-size.json`: TODO: describe the occasion.

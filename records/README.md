# Measurement records

This directory holds every measurement dewasm keeps, one file per run.
Each is a dated JSON record that `cargo xtask record-speed` or `cargo xtask record-size` writes.
A record's suffix names its kind, `-speed.json` or `-size.json`.
The generated documents are [`docs/benchmarks/results.md`](../docs/benchmarks/results.md) and [`docs/sizes/results.md`](../docs/sizes/results.md).
Each is rendered from the newest record of its kind.
`cargo xtask render-speed` renders the first, and `cargo xtask render-size` the second.
An older record is measurement history.
Naming it re-renders it, as in `cargo xtask render-speed records/<file>-speed.json`.

A record names its schema version.
A new schema version ships with an update by `cargo xtask migrate-records`.
That update rewrites every stored record in place.
The commands that read records support only the current schema.

Every record file has one line here saying why it was taken.
A run adds its line with a `TODO`; fill it in when committing the record.

## Speed records

- `2026-08-02T02-41-22Z-speed.json`: the first record (#106).
- `2026-08-03T16-23-15Z-speed.json`: value-addressed branches and the halved generated source (#113).
- `2026-08-11T18-25-24Z-speed.json`: re-baseline after #176, #167/#168 and #195's unit-comment rewrites.
- `2026-08-16T09-22-02Z-speed.json`: re-baseline after the #164 mask elision (#224 to #245).
- `2026-08-22T06-26-23Z-speed.json`: the shortened suite and eight new `wat` cases (#266).
- `2026-08-22T08-27-05Z-speed.json`: re-baseline after the f32 rounding change (#268).
- `2026-08-30T05-11-20Z-speed.json`: the converted-wasm3 runners (#279, #278).
- `2026-08-31T09-49-13Z-speed.json`: the official wasm3 asset (#291) and tail calls (#288-#297).
- `2026-09-13T08-39-51Z-speed.json`: the full 28-runner matrix (#309, #316); `monoruby` SQLite unexcluded.
- `2026-09-17T18-28-43Z-speed.json`: `cowsay` replaced by our own build (#322).
- `2026-09-19T04-41-52Z-speed.json`: `cowsay.wasm` v0.2.0, which measures columns (#326).
- `2026-09-25T05-34-26Z-speed.json`: the Spinel runner joins, making 29 (#335).

## Size records

- `2026-08-06T02-31-05Z-size.json`: the first size record (#166).
- `2026-08-06T03-52-05Z-size.json`: the smaller generated source (#167).
- `2026-08-06T04-31-17Z-size.json`: Ruby parenthesis elision (#168).
- `2026-08-11T18-27-09Z-size.json`: re-baseline beside the same day's speed record.
- `2026-08-16T09-23-36Z-size.json`: the #164 reductions: Ruby and Python outputs get smaller.
- `2026-09-13T09-06-26Z-size.json`: re-baseline beside the 28-runner speed record.
- `2026-09-17T18-31-02Z-size.json`: re-baseline beside the `cowsay` replacement (#322).
- `2026-09-19T02-25-54Z-size.json`: re-baseline beside the `cowsay.wasm` v0.2.0 speed record.

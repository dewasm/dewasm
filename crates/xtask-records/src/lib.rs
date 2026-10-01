//! The measurement records under `records/` and the commands that write, render, and update them.
//!
//! `record-speed` runs every dewasm backend against Wasmtime.
//! It also runs them against the wasm interpreters written in the same host languages.
//! `record-size` compares, per app, the wasm binary against every backend's converted source.
//! It lists the installed size of each native runtime beside them.
//! Each writes a dated record under `records/` and renders nothing.
//! `render-speed` and `render-size` turn a record into a results page.
//! Those are `docs/benchmarks/results.md` and `docs/sizes/results.md`.
//! `migrate-records` updates every record to its kind's current schema.
//! None of those outputs is a compared snapshot.
//! Neither a timing nor an installed size is reproducible.
//! So no freshness test guards them.

mod bench;
mod migrate;
mod size;

pub use bench::{record as record_speed, render as render_speed};
pub use migrate::run as migrate_records;
pub use size::{record as record_size, render as render_size};

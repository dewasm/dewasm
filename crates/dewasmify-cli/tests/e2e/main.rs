//! End-to-end tests: convert wasm/wat to Ruby/Bash and run the result
//! with the real interpreters. Hand-written `.wat` fixtures (under
//! `examples/wat/`) are checked against fixed expected output — split by
//! scenario shape into `standalone` (no glue, one table for both
//! languages), `library` (per-language glue over one shared table), and
//! `ruby` (scenarios with no Bash counterpart: provider objects, WASI
//! filesystem). Real-world binaries (`apps`, fetched by
//! `examples/apps/fetch.sh`, ADR-9) are checked against golden output
//! captured from wasmtime (ADR-15). All are one cargo test target since
//! they're the same kind of test with a different fixture source; shared
//! plumbing lives in `support`.

mod apps;
mod library;
mod ruby;
mod standalone;
mod support;

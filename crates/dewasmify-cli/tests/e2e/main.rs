//! End-to-end tests: convert wasm/wat to Ruby/Bash and run the result
//! with the real interpreters, checked either against a fixed expected
//! output (`scripted`, hand-written `.wat` fixtures under
//! `examples/wat/`) or against real `wasmtime` running the same
//! real-world binary (`apps`, fetched by `examples/apps/fetch.sh`, ADR-9).
//! Both are one cargo test target since they're the same kind of test
//! with a different fixture source; shared plumbing lives in `support`.

mod apps;
mod scripted;
mod support;

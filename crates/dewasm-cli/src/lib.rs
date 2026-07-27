//! Library surface of the `dewasm-cli` crate. The `dewasm` binary
//! (`src/main.rs`) is the primary product; this library exists so that code
//! needing every backend at once — currently only the `docs/support.md`
//! rendering — has a single shared home instead of being duplicated between
//! the `support_docs` test and `cargo xtask update-support-docs`.

pub mod support_docs;

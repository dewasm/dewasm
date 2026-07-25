//! dewasmify-core: decode + validate wasm binaries and build the structured
//! IR shared by all language backends.

pub mod feature;
pub mod ir;

mod func;
mod module;

pub use module::{build_module, features, is_component};

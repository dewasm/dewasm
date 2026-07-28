//! dewasm-core: decode + validate wasm binaries and build the structured
//! IR shared by all language backends.

pub mod feature;
pub mod ir;

mod debug_line;
mod func;
mod module;

pub use module::{build_module, build_module_with_options, features, is_component, BuildOptions};

//! dewasmify-core: decode + validate wasm binaries and build the structured
//! IR shared by all language backends.

pub mod canon;
pub mod component;
pub mod feature;
pub mod ir;

mod func;
mod module;

pub use component::{build_component, is_component};
pub use module::{build_module, features};

//! Feature taxonomy for the support matrix (ADR-8).
//!
//! Every "unsupported" conversion error is attributed to one or more
//! `Feature`s so the spec harness can tell declared gaps from regressions,
//! and so docs/support.md can be generated from code.

use std::fmt;

use wasmparser::WasmFeatures;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Feature {
    // Wasm 1.0 core capabilities dewasmify has not implemented yet
    // (explicit debt, first in line on the roadmap).
    ImportedGlobals,
    ImportedMemories,
    ImportedTables,
    MultipleTables,
    /// The table half of bulk memory: passive/declared element segments,
    /// expression element items, table.init/copy, elem.drop. (The memory
    /// half is supported.)
    TableBulkOps,
    /// f32/f64 values and operations. Core wasm 1.0, but a backend whose
    /// language has no usable floats (Bash, until the ADR-5 softfloat
    /// lands) refuses float-using modules at conversion time.
    Floats,
    // Post-1.0 proposals.
    ReferenceTypes,
    FunctionReferences,
    Gc,
    MultiMemory,
    ExtendedConst,
    Simd,
    RelaxedSimd,
    Threads,
    TailCall,
    Memory64,
    ExceptionHandling,
    WideArithmetic,
    CustomPageSizes,
}

impl Feature {
    pub const ALL: &[Feature] = &[
        Feature::ImportedGlobals,
        Feature::ImportedMemories,
        Feature::ImportedTables,
        Feature::MultipleTables,
        Feature::TableBulkOps,
        Feature::Floats,
        Feature::ReferenceTypes,
        Feature::FunctionReferences,
        Feature::Gc,
        Feature::MultiMemory,
        Feature::ExtendedConst,
        Feature::Simd,
        Feature::RelaxedSimd,
        Feature::Threads,
        Feature::TailCall,
        Feature::Memory64,
        Feature::ExceptionHandling,
        Feature::WideArithmetic,
        Feature::CustomPageSizes,
    ];

    pub fn from_id(id: &str) -> Option<Feature> {
        Feature::ALL.iter().copied().find(|f| f.id() == id)
    }

    /// Stable kebab-case id used in harness reports and docs.
    pub fn id(self) -> &'static str {
        match self {
            Feature::ImportedGlobals => "imported-globals",
            Feature::ImportedMemories => "imported-memories",
            Feature::ImportedTables => "imported-tables",
            Feature::MultipleTables => "multiple-tables",
            Feature::TableBulkOps => "table-bulk-ops",
            Feature::Floats => "floats",
            Feature::ReferenceTypes => "reference-types",
            Feature::FunctionReferences => "function-references",
            Feature::Gc => "gc",
            Feature::MultiMemory => "multi-memory",
            Feature::ExtendedConst => "extended-const",
            Feature::Simd => "simd",
            Feature::RelaxedSimd => "relaxed-simd",
            Feature::Threads => "threads",
            Feature::TailCall => "tail-call",
            Feature::Memory64 => "memory64",
            Feature::ExceptionHandling => "exception-handling",
            Feature::WideArithmetic => "wide-arithmetic",
            Feature::CustomPageSizes => "custom-page-sizes",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Feature::ImportedGlobals => "Imported globals (wasm 1.0)",
            Feature::ImportedMemories => "Imported memories (wasm 1.0)",
            Feature::ImportedTables => "Imported tables (wasm 1.0)",
            Feature::MultipleTables => "Multiple tables",
            Feature::TableBulkOps => "Bulk table ops / passive element segments",
            Feature::Floats => "Floating-point (wasm 1.0)",
            Feature::ReferenceTypes => "Reference types",
            Feature::FunctionReferences => "Typed function references",
            Feature::Gc => "Garbage collection",
            Feature::MultiMemory => "Multiple memories",
            Feature::ExtendedConst => "Extended constant expressions",
            Feature::Simd => "SIMD",
            Feature::RelaxedSimd => "Relaxed SIMD",
            Feature::Threads => "Threads and atomics",
            Feature::TailCall => "Tail calls",
            Feature::Memory64 => "64-bit memory",
            Feature::ExceptionHandling => "Exception handling",
            Feature::WideArithmetic => "Wide arithmetic",
            Feature::CustomPageSizes => "Custom page sizes",
        }
    }

    /// Validator feature bits that this proposal gates, for attributing
    /// validation failures. `None` for capabilities that validate fine
    /// under the base feature set and are rejected during IR building.
    pub fn validator_bits(self) -> Option<WasmFeatures> {
        Some(match self {
            Feature::FunctionReferences => WasmFeatures::FUNCTION_REFERENCES,
            Feature::Gc => WasmFeatures::GC | WasmFeatures::FUNCTION_REFERENCES,
            Feature::MultiMemory => WasmFeatures::MULTI_MEMORY,
            Feature::ExtendedConst => WasmFeatures::EXTENDED_CONST,
            Feature::Simd => WasmFeatures::SIMD,
            Feature::RelaxedSimd => WasmFeatures::RELAXED_SIMD | WasmFeatures::SIMD,
            Feature::Threads => WasmFeatures::THREADS,
            Feature::TailCall => WasmFeatures::TAIL_CALL,
            Feature::Memory64 => WasmFeatures::MEMORY64,
            Feature::ExceptionHandling => WasmFeatures::EXCEPTIONS,
            Feature::WideArithmetic => WasmFeatures::WIDE_ARITHMETIC,
            Feature::CustomPageSizes => WasmFeatures::CUSTOM_PAGE_SIZES,
            _ => return None,
        })
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// A conversion refusal attributed to declared-unsupported features.
/// Anything the converter rejects *without* this attribution is treated
/// as a bug by the spec harness.
#[derive(Debug)]
pub struct UnsupportedError {
    pub features: Vec<Feature>,
    pub detail: String,
}

impl UnsupportedError {
    pub fn new(feature: Feature, detail: impl Into<String>) -> Self {
        UnsupportedError {
            features: vec![feature],
            detail: detail.into(),
        }
    }
}

impl fmt::Display for UnsupportedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<&str> = self.features.iter().map(|f| f.id()).collect();
        write!(f, "unsupported ({}): {}", ids.join("+"), self.detail)
    }
}

impl std::error::Error for UnsupportedError {}

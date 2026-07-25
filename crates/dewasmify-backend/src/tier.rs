//! Backend support tiers (ADR-23): a Zig-style ladder — Tier 1 is the
//! strongest — specialized to wasm 1.0 + WASI preview 1. Post-1.0
//! proposals and the component model are orthogonal per-backend extension
//! badges tracked by the Features matrix; they never affect a tier.

use std::fmt;

use dewasmify_core::feature::Feature;

use crate::{Backend, SupportStatus, WASI_PREVIEW1_FUNCTIONS};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    /// Full: wasm 1.0 + WASI p1 handled completely; nothing in a p1
    /// binary fails short of the out-of-scope socket surface.
    Tier1,
    /// Production: real-world p1 binaries from the major toolchains
    /// convert and run at production quality. The standard goal for new
    /// backends.
    Tier2,
    /// Core: self-contained wasm 1.0 CLI binaries (imports limited to
    /// WASI functions) convert and run.
    Tier3,
    /// Experimental: under development, no guarantees; wired into the
    /// spec harness but outside the default gate.
    Tier4,
}

impl Tier {
    /// Best first, matching the `Ord` derive (`Tier1 < Tier2 < ...`).
    pub const ALL: &[Tier] = &[Tier::Tier1, Tier::Tier2, Tier::Tier3, Tier::Tier4];

    pub fn number(self) -> u8 {
        match self {
            Tier::Tier1 => 1,
            Tier::Tier2 => 2,
            Tier::Tier3 => 3,
            Tier::Tier4 => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tier::Tier1 => "Full",
            Tier::Tier2 => "Production",
            Tier::Tier3 => "Core",
            Tier::Tier4 => "Experimental",
        }
    }

    /// One-line concept for the generated docs; the detailed requirement
    /// checklists live in ADR-23.
    pub fn summary(self) -> &'static str {
        match self {
            Tier::Tier1 => {
                "wasm 1.0 + WASI p1 handled completely (all p1 functions \
                 short of the out-of-scope socket surface, empty wasm-1.0 \
                 failure ledger)"
            }
            Tier::Tier2 => {
                "real-world p1 binaries from the major toolchains convert \
                 and run at production quality (full wasm 1.0 imports, WASI \
                 filesystem); the standard goal for new backends"
            }
            Tier::Tier3 => {
                "self-contained wasm 1.0 CLI binaries (function imports \
                 only, WASI core) convert and run; at least one real app \
                 verified end to end"
            }
            Tier::Tier4 => {
                "under development, no guarantees; wired into the spec \
                 harness but outside the default gate"
            }
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tier {} ({})", self.number(), self.name())
    }
}

/// The tier whose requirements include `Supported` status for this
/// feature (requirements are cumulative, so a `Tier3` feature is also
/// required at Tier 2 and Tier 1), or `None` for extension badges.
/// Exhaustive on purpose: a new `Feature` variant must decide here
/// whether it joins the ladder or the badge side.
pub fn feature_tier(feature: Feature) -> Option<Tier> {
    match feature {
        Feature::Floats => Some(Tier::Tier3),
        Feature::ImportedGlobals | Feature::ImportedMemories | Feature::ImportedTables => {
            Some(Tier::Tier2)
        }
        // wasm 2.0+ proposals and the component model: extension badges.
        Feature::MultipleTables
        | Feature::TableBulkOps
        | Feature::ReferenceTypes
        | Feature::FunctionReferences
        | Feature::Gc
        | Feature::MultiMemory
        | Feature::ExtendedConst
        | Feature::Simd
        | Feature::RelaxedSimd
        | Feature::Threads
        | Feature::TailCall
        | Feature::Memory64
        | Feature::ExceptionHandling
        | Feature::WideArithmetic
        | Feature::CustomPageSizes
        | Feature::ComponentModel => None,
    }
}

/// Whether the feature is an extension badge rather than part of the
/// wasm 1.0 tier ladder.
pub fn is_extension(feature: Feature) -> bool {
    feature_tier(feature).is_none()
}

/// The statically checkable requirements of `tier` that `backend` does
/// not meet, as human-readable gap descriptions. The dynamic requirements
/// (spec sweep results, e2e cases, lowering ADRs) are enforced by the
/// test suite itself; ADR-23 records the split.
pub fn tier_gaps(backend: &dyn Backend, tier: Tier) -> Vec<String> {
    let mut gaps = Vec::new();
    if tier == Tier::Tier4 {
        return gaps;
    }
    for feature in Feature::ALL {
        let required = feature_tier(*feature).is_some_and(|at| at >= tier);
        if required && backend.feature_status(*feature) != SupportStatus::Supported {
            gaps.push(format!("feature {} is not Supported", feature.id()));
        }
    }
    for (name, wasi_tier) in WASI_PREVIEW1_FUNCTIONS {
        let required = wasi_tier.is_some_and(|at| at >= tier);
        if required && !backend.has_wasi_p1(name) {
            gaps.push(format!("WASI p1 {name} has no runtime unit"));
        }
    }
    if tier == Tier::Tier1 && !backend.wasm10_ledger_clean() {
        gaps.push("spec EXPECTED_FAILURES ledger still holds wasm 1.0 entries".to_string());
    }
    gaps
}

/// The best tier whose statically checkable requirements the backend
/// meets. `Tier4` is the floor for any backend wired into the harness.
pub fn achieved_tier(backend: &dyn Backend) -> Tier {
    for tier in [Tier::Tier1, Tier::Tier2, Tier::Tier3] {
        if tier_gaps(backend, tier).is_empty() {
            return tier;
        }
    }
    Tier::Tier4
}

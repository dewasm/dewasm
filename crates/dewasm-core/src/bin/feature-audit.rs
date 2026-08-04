//! Report, per wasm binary, the minimal set of post-baseline proposals it needs to validate, plus its WASI preview 1 import surface.
//!
//! This is the ADR-24 app audit test: before an app is pinned as a conversion target, run this tool on its binary; an app that needs a proposal outside the 0.1 scope (wasm 1.0 + the universally-emitted baseline) is deferred and documented in `docs/apps-audit.md`.
//!
//! Deliberately built on raw `wasmparser::WasmFeatures` bits rather than the crate's `Feature` enum, so it keeps naming proposals the converter itself no longer models.
//!
//! Usage: cargo run -p dewasm-core --bin feature-audit -- <file.wasm>...

use std::collections::BTreeMap;
use std::process::ExitCode;

use wasmparser::{Parser, Payload, TypeRef, Validator, WasmFeatures};

/// The 0.1 input scope of ADR-24: wasm 1.0 plus the baseline extensions every current toolchain emits.
fn baseline() -> WasmFeatures {
    WasmFeatures::WASM1
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
}

/// Every post-baseline proposal this toolchain's validator knows, with the extra bits it implies (e.g. relaxed-simd needs simd).
fn proposals() -> Vec<(&'static str, WasmFeatures)> {
    vec![
        ("reference-types", WasmFeatures::REFERENCE_TYPES),
        ("tail-call", WasmFeatures::TAIL_CALL),
        ("exception-handling", WasmFeatures::EXCEPTIONS),
        ("simd", WasmFeatures::SIMD),
        (
            "relaxed-simd",
            WasmFeatures::RELAXED_SIMD | WasmFeatures::SIMD,
        ),
        ("threads", WasmFeatures::THREADS),
        ("gc", WasmFeatures::GC | WasmFeatures::FUNCTION_REFERENCES),
        ("function-references", WasmFeatures::FUNCTION_REFERENCES),
        ("multi-memory", WasmFeatures::MULTI_MEMORY),
        ("extended-const", WasmFeatures::EXTENDED_CONST),
        ("memory64", WasmFeatures::MEMORY64),
        ("wide-arithmetic", WasmFeatures::WIDE_ARITHMETIC),
        ("custom-page-sizes", WasmFeatures::CUSTOM_PAGE_SIZES),
    ]
}

fn validates(bytes: &[u8], extra: &[(&str, WasmFeatures)]) -> bool {
    let bits = extra.iter().fold(baseline(), |acc, (_, b)| acc | *b);
    Validator::new_with_features(bits)
        .validate_all(bytes)
        .is_ok()
}

/// Greedy minimization, same shape as `classify_validation_failure` in `module.rs`: start from all known proposals, drop any whose absence still validates.
fn minimal_proposals(bytes: &[u8]) -> Option<Vec<&'static str>> {
    let mut active = proposals();
    if !validates(bytes, &active) {
        return None;
    }
    for i in (0..active.len()).rev() {
        let mut without = active.clone();
        without.remove(i);
        if validates(bytes, &without) {
            active = without;
        }
    }
    Some(active.into_iter().map(|(name, _)| name).collect())
}

/// Imports grouped by module, so the WASI p1 surface an app actually uses is visible next to the feature verdict.
fn import_surface(bytes: &[u8]) -> BTreeMap<String, Vec<String>> {
    let mut by_module: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for payload in Parser::new(0).parse_all(bytes) {
        let Ok(Payload::ImportSection(reader)) = payload else {
            continue;
        };
        for import in reader.into_imports().flatten() {
            if matches!(import.ty, TypeRef::Func(_)) {
                by_module
                    .entry(import.module.to_string())
                    .or_default()
                    .push(import.name.to_string());
            }
        }
    }
    for names in by_module.values_mut() {
        names.sort();
        names.dedup();
    }
    by_module
}

/// LLVM keeps `call_indirect` immediates as padded (overlong) LEBs when reference-types is enabled, so wasip1 binaries from clang/zig/rustc commonly *validate* only with the reference-types bit while using no construct from the proposal. Distinguish that encoding artifact from a real use: return a description of the first genuine reference-types construct, or `None` if the module is MVP-shaped.
fn first_ref_types_construct(bytes: &[u8]) -> Option<String> {
    use wasmparser::{CompositeInnerType, Operator, ValType};
    let is_ref = |ty: &ValType| matches!(ty, ValType::Ref(_));
    let mut tables = 0u32;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.ok()? {
            Payload::TypeSection(reader) => {
                for group in reader.into_iter().flatten() {
                    for sub in group.into_types() {
                        if let CompositeInnerType::Func(f) = &sub.composite_type.inner {
                            if f.params().iter().any(&is_ref) || f.results().iter().any(&is_ref) {
                                return Some("a reference type used as a value type".into());
                            }
                        }
                    }
                }
            }
            Payload::TableSection(reader) => {
                for table in reader.into_iter().flatten() {
                    tables += 1;
                    if table.ty.element_type != wasmparser::RefType::FUNCREF {
                        return Some("a non-funcref table".into());
                    }
                }
                if tables > 1 {
                    return Some("multiple tables".into());
                }
            }
            Payload::GlobalSection(reader) => {
                for global in reader.into_iter().flatten() {
                    if is_ref(&global.ty.content_type) {
                        return Some("a reference-typed global".into());
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut locals = body.get_locals_reader().ok()?;
                for _ in 0..locals.get_count() {
                    let (_, ty) = locals.read().ok()?;
                    if is_ref(&ty) {
                        return Some("a reference-typed local".into());
                    }
                }
                for op in body.get_operators_reader().ok()? {
                    match op.ok()? {
                        Operator::TableGet { .. }
                        | Operator::TableSet { .. }
                        | Operator::TableGrow { .. }
                        | Operator::TableSize { .. }
                        | Operator::TableFill { .. } => {
                            return Some("a table instruction".into());
                        }
                        Operator::RefNull { .. }
                        | Operator::RefIsNull
                        | Operator::RefFunc { .. } => {
                            return Some("a ref instruction".into());
                        }
                        Operator::TypedSelect { .. } | Operator::TypedSelectMulti { .. } => {
                            return Some("a typed select".into());
                        }
                        Operator::CallIndirect { table_index, .. } if table_index != 0 => {
                            return Some("call_indirect on a non-zero table".into());
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn is_component(bytes: &[u8]) -> bool {
    // Core modules carry version 1 / layer 0; components layer 1.
    bytes.len() >= 8 && bytes[0..4] == *b"\0asm" && bytes[6..8] == [0x01, 0x00]
}

fn audit(path: &str) -> Result<bool, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());

    if is_component(&bytes) {
        println!("{name}: component-model binary (layer 1) — outside the 0.1 scope");
        return Ok(false);
    }

    let clean = match minimal_proposals(&bytes) {
        None => {
            println!("{name}: does not validate even with every known proposal enabled");
            false
        }
        Some(needed) if needed.is_empty() => {
            println!("{name}: baseline only (wasm 1.0 + universal extensions) — in scope");
            true
        }
        Some(needed) => {
            if needed == ["reference-types"] {
                match first_ref_types_construct(&bytes) {
                    None => {
                        println!(
                            "{name}: baseline + reference-types bit for overlong \
                             call_indirect immediates only (no construct used) — in scope"
                        );
                        true
                    }
                    Some(construct) => {
                        println!(
                            "{name}: needs reference-types ({construct}) — outside the 0.1 scope"
                        );
                        false
                    }
                }
            } else {
                println!(
                    "{name}: needs {} — outside the 0.1 scope",
                    needed.join(", ")
                );
                // Name the first offending construct so the audit record can say *why* the proposal is required, not just that it is.
                if let Err(err) = Validator::new_with_features(baseline()).validate_all(&bytes) {
                    println!("  first offense under baseline: {err}");
                }
                false
            }
        }
    };

    for (module, names) in import_surface(&bytes) {
        if module.starts_with("wasi") {
            println!("  {module} ({}): {}", names.len(), names.join(", "));
        } else {
            println!("  {module}: {} imported functions", names.len());
        }
    }
    Ok(clean)
}

fn main() -> ExitCode {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: feature-audit <file.wasm>...");
        return ExitCode::from(2);
    }
    let mut all_clean = true;
    for path in &paths {
        match audit(path) {
            Ok(clean) => all_clean &= clean,
            Err(err) => {
                eprintln!("{err}");
                return ExitCode::from(2);
            }
        }
    }
    // Nonzero when any binary needs out-of-scope features, so scripts can branch on the verdict; the human decision (defer vs. rethink) is recorded in docs/apps-audit.md.
    if all_clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

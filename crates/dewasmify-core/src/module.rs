//! Decode + validate a wasm binary and build the [`crate::ir::Module`].

use anyhow::{Context, Result};
use wasmparser::{
    CompositeInnerType, ConstExpr, DataKind, ElementItems, ElementKind, ExternalKind, Operator,
    Parser, Payload, TypeRef, Validator, WasmFeatures,
};

use crate::feature::{Feature, UnsupportedError};
use crate::func::FuncBuilder;
use crate::ir;

pub(crate) fn unsupported(feature: Feature, detail: impl Into<String>) -> anyhow::Error {
    UnsupportedError::new(feature, detail).into()
}

/// Wasm feature set accepted by dewasmify's first release: Wasm 1.0 plus the
/// extensions that C/Rust toolchains enable by default and that need no new
/// runtime machinery (sign-extension, saturating truncation, multi-value,
/// memory.copy/fill/init + passive data segments).
pub fn features() -> WasmFeatures {
    // REFERENCE_TYPES is enabled only because modern encoders (e.g. the
    // wast crate) emit encodings gated on it even for MVP-shaped modules;
    // actual reference-type constructs are rejected during IR building.
    WasmFeatures::WASM1
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::REFERENCE_TYPES
}

pub fn build_module(bytes: &[u8]) -> Result<ir::Module> {
    if let Err(err) = Validator::new_with_features(features()).validate_all(bytes) {
        // Attribute the refusal to the proposals whose validator features
        // would make the module validate (ADR-8); an empty feature list
        // means "newer than this toolchain knows".
        let needed = classify_validation_failure(bytes).unwrap_or_default();
        return Err(anyhow::Error::new(UnsupportedError {
            features: needed,
            detail: format!("wasm validation failed: {err}"),
        }));
    }

    let mut module = ir::Module {
        types: Vec::new(),
        imported_funcs: Vec::new(),
        funcs: Vec::new(),
        imported_tables: Vec::new(),
        tables: Vec::new(),
        imported_memory: None,
        memory: None,
        imported_globals: Vec::new(),
        globals: Vec::new(),
        exports: Vec::new(),
        elems: Vec::new(),
        datas: Vec::new(),
        start: None,
    };

    // Type indices of defined functions, in code section order.
    let mut defined_func_types: Vec<u32> = Vec::new();
    let mut next_code_index = 0usize;

    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::TypeSection(reader) => {
                for rec_group in reader {
                    for sub_ty in rec_group?.into_types() {
                        match &sub_ty.composite_type.inner {
                            CompositeInnerType::Func(f) => {
                                module.types.push(ir::FuncType {
                                    params: f
                                        .params()
                                        .iter()
                                        .map(|t| val_type(*t))
                                        .collect::<Result<_>>()?,
                                    results: f
                                        .results()
                                        .iter()
                                        .map(|t| val_type(*t))
                                        .collect::<Result<_>>()?,
                                });
                            }
                            other => {
                                return Err(unsupported(
                                    Feature::Gc,
                                    format!("non-function type {other:?}"),
                                ))
                            }
                        }
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    let detail = format!("import {}.{}", import.module, import.name);
                    match import.ty {
                        TypeRef::Func(type_idx) => {
                            module.imported_funcs.push(ir::ImportedFunc {
                                module: import.module.to_string(),
                                name: import.name.to_string(),
                                type_idx,
                            });
                        }
                        TypeRef::Global(ty) => {
                            module.imported_globals.push(ir::ImportedGlobal {
                                module: import.module.to_string(),
                                name: import.name.to_string(),
                                ty: val_type(ty.content_type)?,
                                mutable: ty.mutable,
                            });
                        }
                        TypeRef::Memory(ty) => {
                            if module.imported_memory.is_some() || module.memory.is_some() {
                                return Err(unsupported(Feature::MultiMemory, "second memory"));
                            }
                            module.imported_memory = Some(ir::ImportedMemory {
                                module: import.module.to_string(),
                                name: import.name.to_string(),
                                min_pages: ty.initial,
                                max_pages: ty.maximum,
                            });
                        }
                        TypeRef::Table(ty) => {
                            module.imported_tables.push(ir::ImportedTable {
                                module: import.module.to_string(),
                                name: import.name.to_string(),
                                min: ty.initial.try_into().context("table too large")?,
                                max: ty
                                    .maximum
                                    .map(|m| m.try_into())
                                    .transpose()
                                    .context("table too large")?,
                            });
                        }
                        TypeRef::Tag(_) => {
                            return Err(unsupported(Feature::ExceptionHandling, detail))
                        }
                        TypeRef::FuncExact(_) => {
                            return Err(unsupported(Feature::FunctionReferences, detail))
                        }
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for ty in reader {
                    defined_func_types.push(ty?);
                }
            }
            Payload::TableSection(reader) => {
                for table in reader {
                    let table = table?;
                    module.tables.push(ir::Table {
                        min: table.ty.initial.try_into().context("table too large")?,
                        max: table
                            .ty
                            .maximum
                            .map(|m| m.try_into())
                            .transpose()
                            .context("table too large")?,
                    });
                }
            }
            Payload::MemorySection(reader) => {
                for mem in reader {
                    let mem = mem?;
                    if module.imported_memory.is_some() || module.memory.is_some() {
                        return Err(unsupported(Feature::MultiMemory, "second memory"));
                    }
                    module.memory = Some(ir::MemoryDef {
                        min_pages: mem.initial,
                        max_pages: mem.maximum,
                    });
                }
            }
            Payload::GlobalSection(reader) => {
                for global in reader {
                    let global = global?;
                    module.globals.push(ir::Global {
                        ty: val_type(global.ty.content_type)?,
                        mutable: global.ty.mutable,
                        init: const_expr(&global.init_expr)?,
                    });
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    let kind = match export.kind {
                        ExternalKind::Func => ir::ExportKind::Func(export.index),
                        ExternalKind::Table => ir::ExportKind::Table(export.index),
                        ExternalKind::Memory => ir::ExportKind::Memory,
                        ExternalKind::Global => ir::ExportKind::Global(export.index),
                        _ => {
                            return Err(unsupported(
                                Feature::ExceptionHandling,
                                format!("export kind for {:?}", export.name),
                            ))
                        }
                    };
                    module.exports.push(ir::Export {
                        name: export.name.to_string(),
                        kind,
                    });
                }
            }
            Payload::StartSection { func, .. } => {
                module.start = Some(func);
            }
            Payload::ElementSection(reader) => {
                for elem in reader {
                    let elem = elem?;
                    let kind = match elem.kind {
                        ElementKind::Active {
                            table_index,
                            offset_expr,
                        } => ir::ElemKind::Active {
                            table_index: table_index.unwrap_or(0),
                            offset: const_expr(&offset_expr)?,
                        },
                        ElementKind::Passive => ir::ElemKind::Passive,
                        ElementKind::Declared => ir::ElemKind::Declared,
                    };
                    let items = match elem.items {
                        ElementItems::Functions(items) => items
                            .into_iter()
                            .map(|i| Ok(Some(i?)))
                            .collect::<Result<Vec<_>>>()?,
                        ElementItems::Expressions(_ref_ty, items) => items
                            .into_iter()
                            .map(|e| elem_item_expr(&e?))
                            .collect::<Result<Vec<_>>>()?,
                    };
                    module.elems.push(ir::ElemSegment { kind, items });
                }
            }
            Payload::DataSection(reader) => {
                for data in reader {
                    let data = data?;
                    let offset = match data.kind {
                        DataKind::Active {
                            memory_index,
                            offset_expr,
                        } => {
                            if memory_index != 0 {
                                return Err(unsupported(
                                    Feature::MultiMemory,
                                    "data segment for a memory other than 0",
                                ));
                            }
                            Some(const_expr(&offset_expr)?)
                        }
                        DataKind::Passive => None,
                    };
                    module.datas.push(ir::DataSegment {
                        offset,
                        data: data.data.to_vec(),
                    });
                }
            }
            Payload::CodeSectionEntry(body) => {
                let type_idx = defined_func_types[next_code_index];
                next_code_index += 1;
                let func = FuncBuilder::new(&module, &defined_func_types, type_idx)
                    .translate(&body)
                    .with_context(|| {
                        format!(
                            "in function #{}",
                            module.imported_funcs.len() + next_code_index - 1
                        )
                    })?;
                module.funcs.push(func);
            }
            _ => {}
        }
    }

    Ok(module)
}

/// Find the minimal set of known proposals that makes `bytes` validate,
/// or `None` if even all of them do not suffice (the module is newer than
/// this toolchain, or genuinely malformed).
fn classify_validation_failure(bytes: &[u8]) -> Option<Vec<Feature>> {
    let candidates: Vec<Feature> = Feature::ALL
        .iter()
        .copied()
        .filter(|f| f.validator_bits().is_some())
        .collect();
    let with = |feats: &[Feature]| {
        let bits = feats
            .iter()
            .filter_map(|f| f.validator_bits())
            .fold(features(), |a, b| a | b);
        Validator::new_with_features(bits)
            .validate_all(bytes)
            .is_ok()
    };
    if !with(&candidates) {
        return None;
    }
    let mut active = candidates;
    for i in (0..active.len()).rev() {
        let mut without: Vec<Feature> = active.clone();
        without.remove(i);
        if with(&without) {
            active = without;
        }
    }
    Some(active)
}

pub(crate) fn val_type(ty: wasmparser::ValType) -> Result<ir::ValType> {
    Ok(match ty {
        wasmparser::ValType::I32 => ir::ValType::I32,
        wasmparser::ValType::I64 => ir::ValType::I64,
        wasmparser::ValType::F32 => ir::ValType::F32,
        wasmparser::ValType::F64 => ir::ValType::F64,
        wasmparser::ValType::V128 => return Err(unsupported(Feature::Simd, "value type v128")),
        _ => {
            return Err(unsupported(
                Feature::ReferenceTypes,
                format!("value type {ty:?}"),
            ))
        }
    })
}

/// Evaluate a constant expression: a plain constant, or (MVP rule) a
/// `global.get` of an imported immutable global — the validator already
/// enforces that constraint, so the index is used as-is.
fn const_expr(expr: &ConstExpr<'_>) -> Result<ir::Expr> {
    let mut reader = expr.get_operators_reader();
    let value = match reader.read()? {
        Operator::I32Const { value } => ir::Expr::I32Const(value as u32),
        Operator::I64Const { value } => ir::Expr::I64Const(value as u64),
        Operator::F32Const { value } => ir::Expr::F32Const(value.bits()),
        Operator::F64Const { value } => ir::Expr::F64Const(value.bits()),
        Operator::GlobalGet { global_index } => ir::Expr::GlobalGet(global_index),
        op => return Err(const_expr_unsupported(&op)),
    };
    match reader.read()? {
        Operator::End => {}
        op => return Err(const_expr_unsupported(&op)),
    }
    Ok(value)
}

fn const_expr_unsupported(op: &Operator<'_>) -> anyhow::Error {
    let feature = match op {
        Operator::RefNull { .. } | Operator::RefFunc { .. } => Feature::ReferenceTypes,
        _ => Feature::ExtendedConst,
    };
    unsupported(feature, format!("constant expression operator {op:?}"))
}

/// One item of an expression-encoded element segment: `ref.func $i` (a
/// function reference) or `ref.null func` (an intentionally-empty slot).
/// Anything else is a genuine reference-types value and stays unsupported.
fn elem_item_expr(expr: &ConstExpr<'_>) -> Result<Option<u32>> {
    let mut reader = expr.get_operators_reader();
    let value = match reader.read()? {
        Operator::RefFunc { function_index } => Some(function_index),
        Operator::RefNull { .. } => None,
        op => {
            return Err(unsupported(
                Feature::ReferenceTypes,
                format!("element item operator {op:?}"),
            ))
        }
    };
    match reader.read()? {
        Operator::End => {}
        op => {
            return Err(unsupported(
                Feature::ReferenceTypes,
                format!("element item operator {op:?}"),
            ))
        }
    }
    Ok(value)
}

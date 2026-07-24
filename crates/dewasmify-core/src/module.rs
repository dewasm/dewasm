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

/// Wasm feature set accepted by the core converter: Wasm 1.0 plus the
/// extensions the shared IR models (sign-extension, saturating truncation,
/// multi-value, bulk memory, reference types). Whether a specific backend
/// lowers a construct is its own declaration (`check_module_support`).
pub fn features() -> WasmFeatures {
    WasmFeatures::WASM1
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::REFERENCE_TYPES
        | WasmFeatures::TAIL_CALL
        | WasmFeatures::EXCEPTIONS
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
        imported_tags: Vec::new(),
        tags: Vec::new(),
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
                                ty: ref_val_type(&ty.element_type)?,
                                min: ty.initial.try_into().context("table too large")?,
                                max: ty
                                    .maximum
                                    .map(|m| m.try_into())
                                    .transpose()
                                    .context("table too large")?,
                            });
                        }
                        TypeRef::Tag(ty) => {
                            module.imported_tags.push(ir::ImportedTag {
                                module: import.module.to_string(),
                                name: import.name.to_string(),
                                type_idx: ty.func_type_idx,
                            });
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
                    if !matches!(table.init, wasmparser::TableInit::RefNull) {
                        return Err(unsupported(
                            Feature::FunctionReferences,
                            "table with an explicit init expression",
                        ));
                    }
                    module.tables.push(ir::Table {
                        ty: ref_val_type(&table.ty.element_type)?,
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
            Payload::TagSection(reader) => {
                for tag in reader {
                    let tag = tag?;
                    module.tags.push(ir::Tag {
                        type_idx: tag.func_type_idx,
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
                        ExternalKind::Tag => ir::ExportKind::Tag(export.index),
                        _ => {
                            return Err(unsupported(
                                Feature::FunctionReferences,
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
                            .map(|i| Ok(ir::ElemItem::Func(i?)))
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
        wasmparser::ValType::Ref(r) => return ref_val_type(&r),
    })
}

/// Map a reference type to the IR: only the two nullable abstract types of
/// the reference-types proposal are representable; everything else is
/// attributed to the proposal that introduced it.
pub(crate) fn ref_val_type(r: &wasmparser::RefType) -> Result<ir::ValType> {
    if *r == wasmparser::RefType::FUNCREF {
        return Ok(ir::ValType::FuncRef);
    }
    if *r == wasmparser::RefType::EXTERNREF {
        return Ok(ir::ValType::ExternRef);
    }
    if *r == wasmparser::RefType::EXNREF {
        return Ok(ir::ValType::ExnRef);
    }
    Err(unsupported(
        heap_type_feature(&r.heap_type()),
        format!("reference type {r:?}"),
    ))
}

pub(crate) fn heap_type_feature(hty: &wasmparser::HeapType) -> Feature {
    use wasmparser::AbstractHeapType as A;
    match hty {
        // Non-nullable/exact forms of these reach here (the nullable ones
        // are handled before); those come from typed function references.
        wasmparser::HeapType::Abstract {
            ty: A::Func | A::Extern,
            ..
        } => Feature::FunctionReferences,
        wasmparser::HeapType::Abstract {
            ty: A::Exn | A::NoExn,
            ..
        } => Feature::ExceptionHandling,
        wasmparser::HeapType::Abstract { .. } => Feature::Gc,
        wasmparser::HeapType::Concrete(_) | wasmparser::HeapType::Exact(_) => {
            Feature::FunctionReferences
        }
    }
}

/// Evaluate a constant expression: a plain constant, a reference constant
/// (`ref.null`/`ref.func`), or (MVP rule) a `global.get` of an imported
/// immutable global — the validator already enforces that constraint, so
/// the index is used as-is.
fn const_expr(expr: &ConstExpr<'_>) -> Result<ir::Expr> {
    let mut reader = expr.get_operators_reader();
    let value = match reader.read()? {
        Operator::I32Const { value } => ir::Expr::I32Const(value as u32),
        Operator::I64Const { value } => ir::Expr::I64Const(value as u64),
        Operator::F32Const { value } => ir::Expr::F32Const(value.bits()),
        Operator::F64Const { value } => ir::Expr::F64Const(value.bits()),
        Operator::GlobalGet { global_index } => ir::Expr::GlobalGet(global_index),
        Operator::RefNull { hty } => ir::Expr::RefNull(heap_val_type(&hty)?),
        Operator::RefFunc { function_index } => ir::Expr::RefFunc(function_index),
        op => return Err(const_expr_unsupported(&op)),
    };
    match reader.read()? {
        Operator::End => {}
        op => return Err(const_expr_unsupported(&op)),
    }
    Ok(value)
}

/// The `ref.null` variant of `ref_val_type`: heap types name the referenced
/// hierarchy directly (nullability is implied by `ref.null`).
pub(crate) fn heap_val_type(hty: &wasmparser::HeapType) -> Result<ir::ValType> {
    use wasmparser::AbstractHeapType as A;
    match hty {
        wasmparser::HeapType::Abstract {
            shared: false,
            ty: A::Func,
        } => Ok(ir::ValType::FuncRef),
        wasmparser::HeapType::Abstract {
            shared: false,
            ty: A::Extern,
        } => Ok(ir::ValType::ExternRef),
        wasmparser::HeapType::Abstract {
            shared: false,
            ty: A::Exn,
        } => Ok(ir::ValType::ExnRef),
        _ => Err(unsupported(
            heap_type_feature(hty),
            format!("heap type {hty:?}"),
        )),
    }
}

fn const_expr_unsupported(op: &Operator<'_>) -> anyhow::Error {
    unsupported(
        Feature::ExtendedConst,
        format!("constant expression operator {op:?}"),
    )
}

/// One item of an expression-encoded element segment: `ref.func $i`,
/// `ref.null` (an intentionally-empty slot), or `global.get $i` of a
/// ref-typed immutable global. Anything else comes from extended constant
/// expressions (or a proposal the validator would have refused first).
fn elem_item_expr(expr: &ConstExpr<'_>) -> Result<ir::ElemItem> {
    let mut reader = expr.get_operators_reader();
    let value = match reader.read()? {
        Operator::RefFunc { function_index } => ir::ElemItem::Func(function_index),
        Operator::RefNull { .. } => ir::ElemItem::Null,
        Operator::GlobalGet { global_index } => ir::ElemItem::Global(global_index),
        op => {
            return Err(unsupported(
                Feature::ExtendedConst,
                format!("element item operator {op:?}"),
            ))
        }
    };
    match reader.read()? {
        Operator::End => {}
        op => {
            return Err(unsupported(
                Feature::ExtendedConst,
                format!("element item operator {op:?}"),
            ))
        }
    }
    Ok(value)
}

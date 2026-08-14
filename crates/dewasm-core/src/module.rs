//! Decode + validate a wasm binary and build the [`crate::ir::Module`].

use std::collections::HashMap;

use anyhow::{Context, Result};
use wasmparser::{
    CompositeInnerType, ConstExpr, DataKind, ElementItems, ElementKind, ExternalKind, Operator,
    Parser, Payload, TypeRef, Validator, WasmFeatures,
};

use crate::debug_line::LineTable;
use crate::feature::{Feature, UnsupportedError};
use crate::func::FuncBuilder;
use crate::ir;

/// Knobs for [`build_module_with_options`].
/// Defaults reproduce [`build_module`] exactly, so every existing call site is byte-identical.
#[derive(Debug, Default, Clone)]
pub struct BuildOptions {
    /// Parse `.debug_*` custom sections and emit [`ir::Stmt::SourceLine`] markers at source-line change points.
    /// Off by default.
    pub debug_line: bool,
}

pub(crate) fn unsupported(feature: Feature, detail: impl Into<String>) -> anyhow::Error {
    UnsupportedError::new(feature, detail).into()
}

/// Wasm feature set accepted by the core converter: Wasm 1.0 plus the universally-emitted baseline (sign-extension, saturating truncation, multi-value, bulk memory) plus exception handling.
/// The `REFERENCE_TYPES` bit is kept purely as an *encoding relaxation*: LLVM toolchains emit overlong `call_indirect` immediates when the reference-types target feature is on, so real wasip1 binaries only validate with the bit, but every actual reference-types construct is rejected during IR building.
/// `LEGACY_EXCEPTIONS` stays off: the retired `try`/`catch`/`rethrow`/`delegate` encoding is refused at validation, so only the `try_table` design reaches the IR.
/// Whether a specific backend lowers a construct is its own declaration (`check_module_support`).
pub fn features() -> WasmFeatures {
    WasmFeatures::WASM1
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::REFERENCE_TYPES
        | WasmFeatures::EXCEPTIONS
}

/// Whether `bytes` is a component-model binary (layer 1) rather than a core module (layer 0).
/// Core modules carry version 1 / layer 0 in bytes 4..8; components use layer 1 (`[.., 0x01, 0x00]`).
/// Used to reject components with a clear error.
pub fn is_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[0..4] == *b"\0asm" && bytes[6..8] == [0x01, 0x00]
}

pub fn build_module(bytes: &[u8]) -> Result<ir::Module> {
    build_module_with_options(bytes, &BuildOptions::default())
}

/// Build the line table in a pre-pass of its own, because `.debug_line` custom sections normally appear *after* the code section: the table must exist before any function body is translated.
fn collect_line_table(bytes: &[u8]) -> Result<Option<LineTable>> {
    let mut debug_sections: HashMap<String, Vec<u8>> = HashMap::new();
    let mut code_section_start: u64 = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::CustomSection(reader) if reader.name().starts_with(".debug_") => {
                debug_sections.insert(reader.name().to_string(), reader.data().to_vec());
            }
            Payload::CodeSectionStart { range, .. } => {
                code_section_start = range.start as u64;
            }
            _ => {}
        }
    }
    LineTable::parse(&debug_sections, code_section_start)
}

pub fn build_module_with_options(bytes: &[u8], options: &BuildOptions) -> Result<ir::Module> {
    if let Err(err) = Validator::new_with_features(features()).validate_all(bytes) {
        // Attribute the refusal to the proposals whose validator features would make the module validate; an empty feature list means "newer than this toolchain knows".
        let needed = classify_validation_failure(bytes).unwrap_or_default();
        return Err(anyhow::Error::new(UnsupportedError {
            features: needed,
            detail: format!("wasm validation failed: {err}"),
        }));
    }

    let line_table = if options.debug_line {
        collect_line_table(bytes)?
    } else {
        None
    };

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
        debug_files: line_table
            .as_ref()
            .map(|t| t.files.clone())
            .unwrap_or_default(),
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
                                ty: table_elem_type(&ty.element_type)?,
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
                        ty: table_elem_type(&table.ty.element_type)?,
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
                    module.tags.push(ir::Tag {
                        type_idx: tag?.func_type_idx,
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
                    .with_line_table(line_table.as_ref())
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

    // Collapse runs of adjacent active data segments into single blobs.
    // Semantics-preserving and unconditional: every backend emits one initializer per active segment, so the reduction composes downstream.
    crate::data_merge::merge_adjacent_data_segments(&mut module);

    Ok(module)
}

/// Find the minimal set of known proposals that makes `bytes` validate, or `None` if even all of them do not suffice (the module is newer than this toolchain, or genuinely malformed).
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

/// Map a wasm value type (as it appears in signatures, locals, globals, and block types) to the IR.
/// `exnref` is the one reference type legal here: `catch_ref` hands a caught exception to ordinary locals, temps, and block types.
/// The reference-types hierarchies are not: funcref stays representable only via `table_elem_type` for a table's element typing.
pub(crate) fn val_type(ty: wasmparser::ValType) -> Result<ir::ValType> {
    Ok(match ty {
        wasmparser::ValType::I32 => ir::ValType::I32,
        wasmparser::ValType::I64 => ir::ValType::I64,
        wasmparser::ValType::F32 => ir::ValType::F32,
        wasmparser::ValType::F64 => ir::ValType::F64,
        wasmparser::ValType::V128 => return Err(unsupported(Feature::Simd, "value type v128")),
        wasmparser::ValType::Ref(r) if r == wasmparser::RefType::EXNREF => ir::ValType::ExnRef,
        wasmparser::ValType::Ref(r) => {
            let feature =
                if r == wasmparser::RefType::FUNCREF || r == wasmparser::RefType::EXTERNREF {
                    Feature::ReferenceTypes
                } else {
                    heap_type_feature(&r.heap_type())
                };
            return Err(unsupported(
                feature,
                format!("reference type {r:?} used as a value type"),
            ));
        }
    })
}

/// Map a table's element type to the IR.
/// Only funcref tables are supported; externref (and every other reference type) is rejected with the proposal that introduced it.
pub(crate) fn table_elem_type(r: &wasmparser::RefType) -> Result<ir::ValType> {
    if *r == wasmparser::RefType::FUNCREF {
        return Ok(ir::ValType::FuncRef);
    }
    let feature = if *r == wasmparser::RefType::EXTERNREF {
        Feature::ReferenceTypes
    } else {
        heap_type_feature(&r.heap_type())
    };
    Err(unsupported(feature, format!("table element type {r:?}")))
}

pub(crate) fn heap_type_feature(hty: &wasmparser::HeapType) -> Feature {
    use wasmparser::AbstractHeapType as A;
    match hty {
        // Non-nullable/exact forms of these reach here (the nullable ones are handled before); those come from typed function references.
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

/// Evaluate a constant expression: a plain constant, or (MVP rule) a `global.get` of an imported immutable global: the validator already enforces that constraint, so the index is used as-is.
/// Reference constants only appear in element items (`elem_item_expr`); a global whose init is a reference constant is rejected earlier by `val_type` refusing its ref-typed content type.
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
    unsupported(
        Feature::ExtendedConst,
        format!("constant expression operator {op:?}"),
    )
}

/// One item of an expression-encoded element segment: `ref.func $i`, `ref.null` (an intentionally-empty slot), or `global.get $i` of a ref-typed immutable global.
/// Anything else comes from extended constant expressions (or a proposal the validator would have refused first).
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

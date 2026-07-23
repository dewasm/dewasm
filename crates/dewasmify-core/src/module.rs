//! Decode + validate a wasm binary and build the [`crate::ir::Module`].

use anyhow::{bail, Context, Result};
use wasmparser::{
    CompositeInnerType, ConstExpr, DataKind, ElementItems, ElementKind, ExternalKind, Operator,
    Parser, Payload, TypeRef, Validator, WasmFeatures,
};

use crate::func::FuncBuilder;
use crate::ir;

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
    Validator::new_with_features(features())
        .validate_all(bytes)
        .context("wasm validation failed")?;

    let mut module = ir::Module {
        types: Vec::new(),
        imported_funcs: Vec::new(),
        funcs: Vec::new(),
        table: None,
        memory: None,
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
                                    params: f.params().iter().map(|t| val_type(*t)).collect::<Result<_>>()?,
                                    results: f.results().iter().map(|t| val_type(*t)).collect::<Result<_>>()?,
                                });
                            }
                            _ => bail!("unsupported non-function type"),
                        }
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import?;
                    match import.ty {
                        TypeRef::Func(type_idx) => {
                            module.imported_funcs.push(ir::ImportedFunc {
                                module: import.module.to_string(),
                                name: import.name.to_string(),
                                type_idx,
                            });
                        }
                        _ => bail!(
                            "unsupported import kind for {}.{}: only function imports are supported",
                            import.module,
                            import.name
                        ),
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
                    if module.table.is_some() {
                        bail!("multiple tables are not supported");
                    }
                    module.table = Some(ir::Table {
                        min: table.ty.initial.try_into().context("table too large")?,
                        max: table.ty.maximum.map(|m| m.try_into()).transpose().context("table too large")?,
                    });
                }
            }
            Payload::MemorySection(reader) => {
                for mem in reader {
                    let mem = mem?;
                    if module.memory.is_some() {
                        bail!("multiple memories are not supported");
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
                        ExternalKind::Table => ir::ExportKind::Table,
                        ExternalKind::Memory => ir::ExportKind::Memory,
                        ExternalKind::Global => ir::ExportKind::Global(export.index),
                        _ => bail!("unsupported export kind for {:?}", export.name),
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
                    let offset = match elem.kind {
                        ElementKind::Active { table_index, offset_expr } => {
                            if table_index.unwrap_or(0) != 0 {
                                bail!("element segments for tables other than 0 are not supported");
                            }
                            const_expr(&offset_expr)?
                        }
                        _ => bail!("passive/declared element segments are not supported"),
                    };
                    let func_indices = match elem.items {
                        ElementItems::Functions(items) => {
                            items.into_iter().collect::<Result<Vec<_>, _>>()?
                        }
                        ElementItems::Expressions(..) => {
                            bail!("element segments with expressions are not supported")
                        }
                    };
                    module.elems.push(ir::ElemSegment { offset, func_indices });
                }
            }
            Payload::DataSection(reader) => {
                for data in reader {
                    let data = data?;
                    let offset = match data.kind {
                        DataKind::Active { memory_index, offset_expr } => {
                            if memory_index != 0 {
                                bail!("data segments for memories other than 0 are not supported");
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
                    .with_context(|| format!("in function #{}", module.imported_funcs.len() + next_code_index - 1))?;
                module.funcs.push(func);
            }
            _ => {}
        }
    }

    Ok(module)
}

pub(crate) fn val_type(ty: wasmparser::ValType) -> Result<ir::ValType> {
    Ok(match ty {
        wasmparser::ValType::I32 => ir::ValType::I32,
        wasmparser::ValType::I64 => ir::ValType::I64,
        wasmparser::ValType::F32 => ir::ValType::F32,
        wasmparser::ValType::F64 => ir::ValType::F64,
        _ => bail!("unsupported value type {ty:?}"),
    })
}

/// Evaluate a constant expression. Only plain constants are supported for
/// now (imported globals are rejected at the import stage already).
fn const_expr(expr: &ConstExpr<'_>) -> Result<ir::Expr> {
    let mut reader = expr.get_operators_reader();
    let value = match reader.read()? {
        Operator::I32Const { value } => ir::Expr::I32Const(value as u32),
        Operator::I64Const { value } => ir::Expr::I64Const(value as u64),
        Operator::F32Const { value } => ir::Expr::F32Const(value.bits()),
        Operator::F64Const { value } => ir::Expr::F64Const(value.bits()),
        op => bail!("unsupported constant expression operator {op:?}"),
    };
    match reader.read()? {
        Operator::End => {}
        op => bail!("unsupported constant expression operator {op:?}"),
    }
    Ok(value)
}

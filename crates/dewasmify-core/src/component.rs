//! Parse a component binary (layer 1) into a component IR: the core
//! modules (each through the existing [`crate::build_module`]), the
//! WIT-level shape of every imported interface, the ordered core
//! instantiation plan, and the canonical adapters (lower/lift/resource
//! -drop) the plan references.
//!
//! Scope (ADR-20): the accepted subset is what `wit-component`-produced
//! `wasi:cli` command components use — N core modules, explicit-argument
//! core instantiations, synthetic (`FromExports`) core instances, `canon
//! lift`/`lower` with utf8 strings, `canon resource.drop`, instance-kind
//! imports, and the trivial nested-component wrapper that re-exports
//! lifted functions as an instance export. Everything else is refused
//! with `UnsupportedError(ComponentModel)` at conversion time (ADR-0).

use anyhow::{Context, Result};
use wasmparser::{
    CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExternalKind, ComponentType,
    ComponentTypeRef, ComponentValType, InstanceTypeDeclaration, Parser, Payload, PrimitiveValType,
    TypeBounds, Validator, WasmFeatures,
};

use crate::feature::{Feature, UnsupportedError};
use crate::ir;

pub(crate) fn unsupported(detail: impl Into<String>) -> anyhow::Error {
    UnsupportedError::new(Feature::ComponentModel, detail).into()
}

/// Whether `bytes` is a component (layer field = 1) rather than a core
/// module.
pub fn is_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[0..4] == b"\0asm" && bytes[6] == 1 && bytes[7] == 0
}

/// A WIT-level value type, fully resolved (no type indices). Resources
/// are identified by a stable `"interface#name"` string.
#[derive(Clone, Debug, PartialEq)]
pub enum WitType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    S8,
    S16,
    S32,
    S64,
    F32,
    F64,
    Char,
    String,
    List(Box<WitType>),
    Record(Vec<(String, WitType)>),
    Tuple(Vec<WitType>),
    Variant(Vec<(String, Option<WitType>)>),
    Enum(Vec<String>),
    Flags(Vec<String>),
    Option(Box<WitType>),
    Result {
        ok: Option<Box<WitType>>,
        err: Option<Box<WitType>>,
    },
    /// An owned resource handle (core i32).
    Own(String),
    /// A borrowed resource handle (core i32).
    Borrow(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct WitFunc {
    pub params: Vec<(String, WitType)>,
    pub result: Option<WitType>,
}

/// A function of an imported interface: the host implements it.
#[derive(Clone, Debug)]
pub struct HostFuncRef {
    /// The import's interface name, e.g. `"wasi:cli/stdout@0.2.9"`.
    pub interface: String,
    /// The function's export name within the interface, e.g.
    /// `"get-stdout"` or `"[method]output-stream.blocking-write-and-flush"`.
    pub func: String,
    pub ty: WitFunc,
}

/// One imported interface (instance-kind component import).
#[derive(Debug)]
pub struct WitImport {
    pub name: String,
    pub funcs: Vec<(String, WitFunc)>,
    /// Resource types the interface exports, as `"interface#name"` ids.
    pub resources: Vec<String>,
}

/// Canonical options of a lift/lower.
#[derive(Clone, Debug, Default)]
pub struct CanonOpts {
    /// Core memory used for lifting/lowering, as (core instance, export
    /// name) — resolved through the core-memory index space.
    pub memory: Option<(usize, String)>,
    /// `cabi_realloc`, same resolution.
    pub realloc: Option<(usize, String)>,
    /// A `post-return` cleanup function to call after lifting results.
    pub post_return: Option<(usize, String)>,
}

/// Something usable where a core function is expected.
#[derive(Clone, Debug)]
pub enum CoreItem {
    /// Export `name` of instantiated core instance `instance`.
    InstanceExport { instance: usize, name: String },
    /// A synthesized adapter calling `host` (canon lower).
    Lower { host: HostFuncRef, opts: CanonOpts },
    /// A synthesized drop for an own handle of `resource`.
    ResourceDrop { resource: String },
}

/// One core instance, in instantiation order.
#[derive(Debug)]
pub enum CoreInstance {
    /// Instantiate `core_modules[module]`; each arg names an instance
    /// whose exports satisfy that import namespace.
    Instantiate {
        module: usize,
        args: Vec<(String, usize)>,
    },
    /// A synthetic instance built from individual items (funcs, and the
    /// shim's re-exported `$imports` table).
    Synthetic(Vec<(String, CoreItem)>),
}

/// A lifted (component-level, host-callable) function.
#[derive(Debug)]
pub struct LiftedFunc {
    pub core_func: CoreItem,
    pub ty: WitFunc,
    pub opts: CanonOpts,
}

#[derive(Debug)]
pub enum ExportItem {
    /// Index into `Component::lifted`.
    Func(usize),
    /// An instance whose exports are lifted functions.
    Instance(Vec<(String, usize)>),
}

#[derive(Debug)]
pub struct ComponentExport {
    pub name: String,
    pub item: ExportItem,
}

#[derive(Debug)]
pub struct Component {
    pub core_modules: Vec<ir::Module>,
    pub imports: Vec<WitImport>,
    /// Core instances in definition order; `Instantiate` entries run in
    /// this order at component instantiation.
    pub instances: Vec<CoreInstance>,
    pub lifted: Vec<LiftedFunc>,
    pub exports: Vec<ComponentExport>,
}

pub fn component_features() -> WasmFeatures {
    crate::module::features() | WasmFeatures::COMPONENT_MODEL
}

pub fn build_component(bytes: &[u8]) -> Result<Component> {
    if let Err(err) = Validator::new_with_features(component_features()).validate_all(bytes) {
        return Err(unsupported(format!("component validation failed: {err}")));
    }
    Walker::default().walk(bytes)
}

// ---- component-level index-space entries -------------------------------

/// A resolved component-type-space entry.
#[derive(Clone, Debug)]
enum CT {
    Defined(WitType),
    Func(WitFunc),
    /// An instance type: export name -> entry (funcs and resource markers).
    Instance(Vec<(String, ShapeEntry)>),
    /// A resource type bound at import time: `"interface#name"`.
    Resource(String),
    /// Anything we don't track further (component types, ...).
    Opaque,
}

/// An entry of an instance *type* before it is bound to an import name:
/// resource identities are local until then (`"<local>#name"` ids).
#[derive(Clone, Debug)]
enum ShapeEntry {
    Func(WitFunc),
    /// A fresh resource introduced by this instance type.
    Resource,
    /// A non-resource type export (e.g. a record like wall-clock's
    /// `datetime`) that other interfaces re-import through type aliases.
    OtherType(CT),
}

/// A component-instance-space entry.
#[derive(Clone, Debug)]
enum CInst {
    /// An imported interface: funcs and resources resolvable by name;
    /// `types` resolves type aliases out of this instance.
    Imported {
        import: usize,
        types: Vec<(String, CT)>,
    },
    /// Result of instantiating a nested wrapper component: export name ->
    /// component-func index.
    Wrapper(Vec<(String, usize)>),
}

/// A component-function-space entry.
#[derive(Clone, Debug)]
enum CFunc {
    /// Function `func` of imported interface `import`.
    Host { import: usize, func: String },
    /// Index into `lifted`.
    Lifted(usize),
}

/// A nested component, accepted only in the wrapper shape: it imports
/// functions and re-exports them (directly or via an inner instance).
#[derive(Debug, Default)]
struct NestedComponent {
    /// Import names in func-index-space order.
    func_imports: Vec<String>,
    /// Component exports: name -> inner func index.
    func_exports: Vec<(String, usize)>,
}

#[derive(Default)]
struct Walker {
    ctypes: Vec<CT>,
    cinstances: Vec<CInst>,
    cfuncs: Vec<CFunc>,
    nested: Vec<NestedComponent>,
    core_funcs: Vec<CoreItem>,
    core_memories: Vec<(usize, String)>,
    core_tables: Vec<(usize, String)>,
    core_globals: Vec<(usize, String)>,
    out: ComponentParts,
}

#[derive(Default)]
struct ComponentParts {
    core_modules: Vec<ir::Module>,
    imports: Vec<WitImport>,
    instances: Vec<CoreInstance>,
    lifted: Vec<LiftedFunc>,
    exports: Vec<ComponentExport>,
}

impl Walker {
    fn walk(mut self, bytes: &[u8]) -> Result<Component> {
        // `parse_all` streams *nested* modules'/components' payloads
        // inline; those are handled from their extracted byte ranges
        // instead, so anything at depth > 0 is skipped here.
        let mut depth = 0usize;
        for payload in Parser::new(0).parse_all(bytes) {
            let payload = payload?;
            if depth > 0 {
                match payload {
                    Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
                    Payload::End(_) => depth -= 1,
                    _ => {}
                }
                continue;
            }
            match payload {
                Payload::Version { .. } | Payload::CustomSection(_) | Payload::End(_) => {}
                Payload::ModuleSection {
                    unchecked_range, ..
                } => {
                    let module = crate::build_module(&bytes[unchecked_range])
                        .context("in a component's core module")?;
                    self.out.core_modules.push(module);
                    depth += 1;
                }
                Payload::ComponentSection {
                    unchecked_range, ..
                } => {
                    let nested = parse_nested_component(&bytes[unchecked_range])?;
                    self.nested.push(nested);
                    depth += 1;
                }
                Payload::ComponentTypeSection(reader) => {
                    for ty in reader {
                        let ct = self.component_type(&ty?)?;
                        self.ctypes.push(ct);
                    }
                }
                Payload::ComponentImportSection(reader) => {
                    for import in reader {
                        let import = import?;
                        self.component_import(import.name.name, &import.ty)?;
                    }
                }
                Payload::ComponentAliasSection(reader) => {
                    for alias in reader {
                        self.component_alias(&alias?)?;
                    }
                }
                Payload::ComponentCanonicalSection(reader) => {
                    for canon in reader {
                        self.canonical(&canon?)?;
                    }
                }
                Payload::InstanceSection(reader) => {
                    for inst in reader {
                        self.core_instance(&inst?)?;
                    }
                }
                Payload::ComponentInstanceSection(reader) => {
                    for inst in reader {
                        self.component_instance(&inst?)?;
                    }
                }
                Payload::ComponentExportSection(reader) => {
                    for export in reader {
                        self.component_export(&export?)?;
                    }
                }
                Payload::ComponentStartSection { .. } => {
                    return Err(unsupported("component start section"));
                }
                other => {
                    return Err(unsupported(format!("component payload {other:?}")));
                }
            }
        }
        Ok(Component {
            core_modules: self.out.core_modules,
            imports: self.out.imports,
            instances: self.out.instances,
            lifted: self.out.lifted,
            exports: self.out.exports,
        })
    }

    // ---- types ----------------------------------------------------------

    fn val_type(&self, ty: &ComponentValType, local: Option<&[CT]>) -> Result<WitType> {
        match ty {
            ComponentValType::Primitive(p) => primitive(p),
            ComponentValType::Type(idx) => {
                let entry = match local {
                    Some(local) => local.get(*idx as usize),
                    None => self.ctypes.get(*idx as usize),
                };
                match entry {
                    Some(CT::Defined(t)) => Ok(t.clone()),
                    Some(CT::Resource(id)) => Ok(WitType::Own(id.clone())),
                    other => Err(unsupported(format!(
                        "value type referencing type entry {other:?}"
                    ))),
                }
            }
        }
    }

    fn defined_type(&self, ty: &ComponentDefinedType<'_>, local: Option<&[CT]>) -> Result<WitType> {
        Ok(match ty {
            ComponentDefinedType::Primitive(p) => primitive(p)?,
            ComponentDefinedType::Record(fields) => WitType::Record(
                fields
                    .iter()
                    .map(|(n, t)| Ok((n.to_string(), self.val_type(t, local)?)))
                    .collect::<Result<_>>()?,
            ),
            ComponentDefinedType::Variant(cases) => WitType::Variant(
                cases
                    .iter()
                    .map(|c| {
                        Ok((
                            c.name.to_string(),
                            c.ty.as_ref().map(|t| self.val_type(t, local)).transpose()?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            ),
            ComponentDefinedType::List(t) => WitType::List(Box::new(self.val_type(t, local)?)),
            ComponentDefinedType::Tuple(ts) => WitType::Tuple(
                ts.iter()
                    .map(|t| self.val_type(t, local))
                    .collect::<Result<_>>()?,
            ),
            ComponentDefinedType::Flags(names) => {
                WitType::Flags(names.iter().map(|s| s.to_string()).collect())
            }
            ComponentDefinedType::Enum(names) => {
                WitType::Enum(names.iter().map(|s| s.to_string()).collect())
            }
            ComponentDefinedType::Option(t) => WitType::Option(Box::new(self.val_type(t, local)?)),
            ComponentDefinedType::Result { ok, err } => WitType::Result {
                ok: ok
                    .as_ref()
                    .map(|t| Ok::<_, anyhow::Error>(Box::new(self.val_type(t, local)?)))
                    .transpose()?,
                err: err
                    .as_ref()
                    .map(|t| Ok::<_, anyhow::Error>(Box::new(self.val_type(t, local)?)))
                    .transpose()?,
            },
            ComponentDefinedType::Own(idx) => WitType::Own(self.resource_id(*idx, local)?),
            ComponentDefinedType::Borrow(idx) => WitType::Borrow(self.resource_id(*idx, local)?),
            other => return Err(unsupported(format!("defined type {other:?}"))),
        })
    }

    fn resource_id(&self, type_idx: u32, local: Option<&[CT]>) -> Result<String> {
        let entry = match local {
            Some(local) => local.get(type_idx as usize),
            None => self.ctypes.get(type_idx as usize),
        };
        match entry {
            Some(CT::Resource(id)) => Ok(id.clone()),
            other => Err(unsupported(format!(
                "handle referencing non-resource type {other:?}"
            ))),
        }
    }

    fn func_type(
        &self,
        ty: &wasmparser::ComponentFuncType<'_>,
        local: Option<&[CT]>,
    ) -> Result<WitFunc> {
        if ty.async_ {
            return Err(unsupported("async function type"));
        }
        Ok(WitFunc {
            params: ty
                .params
                .iter()
                .map(|(n, t)| Ok((n.to_string(), self.val_type(t, local)?)))
                .collect::<Result<_>>()?,
            result: ty
                .result
                .as_ref()
                .map(|t| self.val_type(t, local))
                .transpose()?,
        })
    }

    fn component_type(&mut self, ty: &ComponentType<'_>) -> Result<CT> {
        Ok(match ty {
            ComponentType::Defined(d) => CT::Defined(self.defined_type(d, None)?),
            ComponentType::Func(f) => CT::Func(self.func_type(f, None)?),
            ComponentType::Instance(decls) => CT::Instance(self.instance_shape(decls)?),
            // Component types only describe shapes; nothing consumes them
            // in the accepted subset.
            ComponentType::Component(_) => CT::Opaque,
            ComponentType::Resource { .. } => {
                return Err(unsupported("locally-defined resource type"))
            }
        })
    }

    /// Resolve an instance type's declaration list into (export name ->
    /// entry), with a local type index space. Fresh resources stay
    /// placeholders until the import binds them to an interface name.
    fn instance_shape(
        &self,
        decls: &[InstanceTypeDeclaration<'_>],
    ) -> Result<Vec<(String, ShapeEntry)>> {
        let mut local: Vec<CT> = Vec::new();
        let mut exports: Vec<(String, ShapeEntry)> = Vec::new();
        for decl in decls {
            match decl {
                // Core types live in their own index space, not the
                // component type space.
                InstanceTypeDeclaration::CoreType(_) => {}
                InstanceTypeDeclaration::Type(t) => {
                    let ct = match t {
                        ComponentType::Defined(d) => {
                            CT::Defined(self.defined_type(d, Some(&local))?)
                        }
                        ComponentType::Func(f) => CT::Func(self.func_type(f, Some(&local))?),
                        other => {
                            return Err(unsupported(format!("instance type declaration {other:?}")))
                        }
                    };
                    local.push(ct);
                }
                InstanceTypeDeclaration::Alias(alias) => match alias {
                    ComponentAlias::Outer {
                        count: 1, index, ..
                    } => {
                        let ct = self
                            .ctypes
                            .get(*index as usize)
                            .cloned()
                            .ok_or_else(|| unsupported("outer alias out of range"))?;
                        local.push(ct);
                    }
                    other => return Err(unsupported(format!("instance type alias {other:?}"))),
                },
                InstanceTypeDeclaration::Export { name, ty } => match ty {
                    ComponentTypeRef::Func(idx) => {
                        let f = match local.get(*idx as usize) {
                            Some(CT::Func(f)) => f.clone(),
                            other => {
                                return Err(unsupported(format!(
                                    "instance func export referencing {other:?}"
                                )))
                            }
                        };
                        exports.push((name.name.to_string(), ShapeEntry::Func(f)));
                        // Exports do not extend the local type space.
                    }
                    ComponentTypeRef::Type(TypeBounds::SubResource) => {
                        exports.push((name.name.to_string(), ShapeEntry::Resource));
                        // A fresh resource *does* extend the local type
                        // space; its id is bound at import.
                        local.push(CT::Resource(format!("<local>#{}", name.name)));
                    }
                    ComponentTypeRef::Type(TypeBounds::Eq(idx)) => {
                        let ct = local
                            .get(*idx as usize)
                            .cloned()
                            .ok_or_else(|| unsupported("type export out of range"))?;
                        exports.push((name.name.to_string(), ShapeEntry::OtherType(ct.clone())));
                        local.push(ct);
                    }
                    other => {
                        return Err(unsupported(format!("instance export {other:?}")));
                    }
                },
            }
        }
        Ok(exports)
    }

    // ---- imports --------------------------------------------------------

    fn component_import(&mut self, name: &str, ty: &ComponentTypeRef) -> Result<()> {
        match ty {
            ComponentTypeRef::Instance(type_idx) => {
                let shape = match self.ctypes.get(*type_idx as usize) {
                    Some(CT::Instance(shape)) => shape.clone(),
                    other => {
                        return Err(unsupported(format!(
                            "instance import referencing {other:?}"
                        )))
                    }
                };
                let mut funcs = Vec::new();
                let mut resources = Vec::new();
                let mut types = Vec::new();
                for (export, entry) in &shape {
                    match entry {
                        ShapeEntry::Func(f) => {
                            funcs.push((export.clone(), bind_resources(f, name)))
                        }
                        ShapeEntry::Resource => {
                            let id = format!("{name}#{export}");
                            types.push((export.clone(), CT::Resource(id.clone())));
                            resources.push(id);
                        }
                        ShapeEntry::OtherType(ct) => {
                            types.push((export.clone(), bind_ct(ct, name)));
                        }
                    }
                }
                let import_idx = self.out.imports.len();
                self.out.imports.push(WitImport {
                    name: name.to_string(),
                    funcs,
                    resources,
                });
                self.cinstances.push(CInst::Imported {
                    import: import_idx,
                    types,
                });
                Ok(())
            }
            other => Err(unsupported(format!(
                "component import {name:?} of kind {other:?}"
            ))),
        }
    }

    fn import_func(&self, import: usize, func: &str) -> Result<HostFuncRef> {
        let imp = &self.out.imports[import];
        let ty = imp
            .funcs
            .iter()
            .find(|(n, _)| n == func)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| unsupported(format!("unknown func {func:?} of {:?}", imp.name)))?;
        Ok(HostFuncRef {
            interface: imp.name.clone(),
            func: func.to_string(),
            ty,
        })
    }

    // ---- aliases --------------------------------------------------------

    fn component_alias(&mut self, alias: &ComponentAlias<'_>) -> Result<()> {
        match alias {
            ComponentAlias::InstanceExport {
                kind,
                instance_index,
                name,
            } => {
                let inst = self
                    .cinstances
                    .get(*instance_index as usize)
                    .ok_or_else(|| unsupported("alias instance out of range"))?
                    .clone();
                match (kind, inst) {
                    (ComponentExternalKind::Func, CInst::Imported { import, .. }) => {
                        self.cfuncs.push(CFunc::Host {
                            import,
                            func: name.to_string(),
                        });
                    }
                    (ComponentExternalKind::Func, CInst::Wrapper(exports)) => {
                        let lifted = exports
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, f)| *f)
                            .ok_or_else(|| unsupported("wrapper export not found"))?;
                        self.cfuncs.push(self.cfuncs[lifted].clone());
                    }
                    (ComponentExternalKind::Type, CInst::Imported { types, .. }) => {
                        let ct = types
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, ct)| ct.clone())
                            .ok_or_else(|| {
                                unsupported(format!("type alias of unknown export {name:?}"))
                            })?;
                        self.ctypes.push(ct);
                    }
                    (kind, inst) => {
                        return Err(unsupported(format!("alias of {kind:?} from {inst:?}")))
                    }
                }
                Ok(())
            }
            ComponentAlias::CoreInstanceExport {
                kind,
                instance_index,
                name,
            } => {
                let item = (*instance_index as usize, name.to_string());
                match kind {
                    wasmparser::ExternalKind::Func => {
                        self.core_funcs.push(CoreItem::InstanceExport {
                            instance: item.0,
                            name: item.1,
                        });
                    }
                    wasmparser::ExternalKind::Memory => self.core_memories.push(item),
                    wasmparser::ExternalKind::Table => self.core_tables.push(item),
                    wasmparser::ExternalKind::Global => self.core_globals.push(item),
                    other => {
                        return Err(unsupported(format!("core alias of kind {other:?}")));
                    }
                }
                Ok(())
            }
            ComponentAlias::Outer { .. } => Err(unsupported("outer alias at component level")),
        }
    }

    // ---- canonical functions ---------------------------------------------

    fn canon_opts(&self, options: &[CanonicalOption]) -> Result<CanonOpts> {
        let mut opts = CanonOpts::default();
        for opt in options {
            match opt {
                CanonicalOption::UTF8 => {}
                CanonicalOption::Memory(idx) => {
                    opts.memory = Some(
                        self.core_memories
                            .get(*idx as usize)
                            .cloned()
                            .ok_or_else(|| unsupported("canon memory out of range"))?,
                    );
                }
                CanonicalOption::Realloc(idx) => {
                    let f = self
                        .core_funcs
                        .get(*idx as usize)
                        .ok_or_else(|| unsupported("canon realloc out of range"))?;
                    match f {
                        CoreItem::InstanceExport { instance, name } => {
                            opts.realloc = Some((*instance, name.clone()));
                        }
                        other => {
                            return Err(unsupported(format!("canon realloc via {other:?}")));
                        }
                    }
                }
                CanonicalOption::PostReturn(idx) => {
                    let f = self
                        .core_funcs
                        .get(*idx as usize)
                        .ok_or_else(|| unsupported("canon post-return out of range"))?;
                    match f {
                        CoreItem::InstanceExport { instance, name } => {
                            opts.post_return = Some((*instance, name.clone()));
                        }
                        other => {
                            return Err(unsupported(format!("canon post-return via {other:?}")));
                        }
                    }
                }
                other => {
                    return Err(unsupported(format!("canonical option {other:?}")));
                }
            }
        }
        Ok(opts)
    }

    fn canonical(&mut self, canon: &wasmparser::CanonicalFunction) -> Result<()> {
        use wasmparser::CanonicalFunction as CF;
        match canon {
            CF::Lower {
                func_index,
                options,
            } => {
                let opts = self.canon_opts(options)?;
                let host = match self
                    .cfuncs
                    .get(*func_index as usize)
                    .ok_or_else(|| unsupported("canon lower func out of range"))?
                {
                    CFunc::Host { import, func } => self.import_func(*import, &func.clone())?,
                    CFunc::Lifted(_) => {
                        return Err(unsupported("canon lower of a lifted function"))
                    }
                };
                self.core_funcs.push(CoreItem::Lower { host, opts });
                Ok(())
            }
            CF::Lift {
                core_func_index,
                type_index,
                options,
            } => {
                let opts = self.canon_opts(options)?;
                let core_func = self
                    .core_funcs
                    .get(*core_func_index as usize)
                    .cloned()
                    .ok_or_else(|| unsupported("canon lift core func out of range"))?;
                let ty = match self.ctypes.get(*type_index as usize) {
                    Some(CT::Func(f)) => f.clone(),
                    other => {
                        return Err(unsupported(format!("canon lift type {other:?}")));
                    }
                };
                let idx = self.out.lifted.len();
                self.out.lifted.push(LiftedFunc {
                    core_func,
                    ty,
                    opts,
                });
                self.cfuncs.push(CFunc::Lifted(idx));
                Ok(())
            }
            CF::ResourceDrop { resource } => {
                let id = self.resource_id(*resource, None)?;
                self.core_funcs
                    .push(CoreItem::ResourceDrop { resource: id });
                Ok(())
            }
            other => Err(unsupported(format!("canonical function {other:?}"))),
        }
    }

    // ---- instances --------------------------------------------------------

    fn core_instance(&mut self, inst: &wasmparser::Instance<'_>) -> Result<()> {
        match inst {
            wasmparser::Instance::Instantiate { module_index, args } => {
                let args = args
                    .iter()
                    .map(|a| {
                        // The only core instantiation-arg kind is Instance.
                        (a.name.to_string(), a.index as usize)
                    })
                    .collect();
                self.out.instances.push(CoreInstance::Instantiate {
                    module: *module_index as usize,
                    args,
                });
                Ok(())
            }
            wasmparser::Instance::FromExports(exports) => {
                let mut items = Vec::new();
                for e in exports.iter() {
                    let item = match e.kind {
                        wasmparser::ExternalKind::Func => self
                            .core_funcs
                            .get(e.index as usize)
                            .cloned()
                            .ok_or_else(|| unsupported("synthetic export func out of range"))?,
                        wasmparser::ExternalKind::Table => {
                            let (instance, name) =
                                self.core_tables.get(e.index as usize).cloned().ok_or_else(
                                    || unsupported("synthetic export table out of range"),
                                )?;
                            // A table travels as its defining instance's
                            // export (the shim's `$imports` table).
                            CoreItem::InstanceExport { instance, name }
                        }
                        wasmparser::ExternalKind::Memory => {
                            let (instance, name) = self
                                .core_memories
                                .get(e.index as usize)
                                .cloned()
                                .ok_or_else(|| {
                                    unsupported("synthetic export memory out of range")
                                })?;
                            CoreItem::InstanceExport { instance, name }
                        }
                        wasmparser::ExternalKind::Global => {
                            let (instance, name) = self
                                .core_globals
                                .get(e.index as usize)
                                .cloned()
                                .ok_or_else(|| {
                                    unsupported("synthetic export global out of range")
                                })?;
                            CoreItem::InstanceExport { instance, name }
                        }
                        other => {
                            return Err(unsupported(format!("synthetic export of kind {other:?}")));
                        }
                    };
                    items.push((e.name.to_string(), item));
                }
                self.out.instances.push(CoreInstance::Synthetic(items));
                Ok(())
            }
        }
    }

    fn component_instance(&mut self, inst: &wasmparser::ComponentInstance<'_>) -> Result<()> {
        match inst {
            wasmparser::ComponentInstance::Instantiate {
                component_index,
                args,
            } => {
                let nested = self
                    .nested
                    .get(*component_index as usize)
                    .ok_or_else(|| unsupported("component instantiation out of range"))?;
                // Bind the wrapper's imported funcs to our component funcs.
                let mut bound: Vec<Option<usize>> = vec![None; nested.func_imports.len()];
                for arg in args.iter() {
                    if arg.kind != ComponentExternalKind::Func {
                        return Err(unsupported(format!(
                            "component instantiation arg of kind {:?}",
                            arg.kind
                        )));
                    }
                    let slot = nested
                        .func_imports
                        .iter()
                        .position(|n| n == arg.name)
                        .ok_or_else(|| unsupported("wrapper arg name not imported"))?;
                    let lifted = match self
                        .cfuncs
                        .get(arg.index as usize)
                        .ok_or_else(|| unsupported("wrapper arg func out of range"))?
                    {
                        CFunc::Lifted(idx) => *idx,
                        CFunc::Host { .. } => {
                            return Err(unsupported("wrapper arg is a host import"))
                        }
                    };
                    bound[slot] = Some(lifted);
                }
                let exports = nested
                    .func_exports
                    .iter()
                    .map(|(name, inner)| {
                        let lifted = bound
                            .get(*inner)
                            .copied()
                            .flatten()
                            .ok_or_else(|| unsupported("wrapper export unbound"))?;
                        Ok((name.clone(), lifted))
                    })
                    .collect::<Result<Vec<_>>>()?;
                // Store wrapper exports as *lifted indices*; register the
                // instance and matching cfunc entries for later aliases.
                let cfunc_exports = exports
                    .iter()
                    .map(|(n, lifted)| {
                        // Position of a synthetic cfunc if aliased later.
                        (n.clone(), self.lifted_cfunc(*lifted))
                    })
                    .collect();
                self.cinstances.push(CInst::Wrapper(cfunc_exports));
                Ok(())
            }
            wasmparser::ComponentInstance::FromExports(_) => {
                Err(unsupported("component instance from exports"))
            }
        }
    }

    /// A `cfuncs` index that resolves to `lifted`; used when a wrapper
    /// instance's exports are aliased.
    fn lifted_cfunc(&mut self, lifted: usize) -> usize {
        // cfuncs entries are cheap; push a dedicated one.
        self.cfuncs.push(CFunc::Lifted(lifted));
        self.cfuncs.len() - 1
    }

    // ---- exports ----------------------------------------------------------

    fn component_export(&mut self, export: &wasmparser::ComponentExport<'_>) -> Result<()> {
        let name = export.name.name.to_string();
        match export.kind {
            ComponentExternalKind::Func => {
                let lifted = match self
                    .cfuncs
                    .get(export.index as usize)
                    .ok_or_else(|| unsupported("export func out of range"))?
                {
                    CFunc::Lifted(idx) => *idx,
                    CFunc::Host { .. } => {
                        return Err(unsupported("re-exporting an imported function"))
                    }
                };
                // Exports extend the func index space with themselves.
                self.cfuncs.push(CFunc::Lifted(lifted));
                self.out.exports.push(ComponentExport {
                    name,
                    item: ExportItem::Func(lifted),
                });
                Ok(())
            }
            ComponentExternalKind::Instance => {
                let inst = self
                    .cinstances
                    .get(export.index as usize)
                    .cloned()
                    .ok_or_else(|| unsupported("export instance out of range"))?;
                let exports = match inst {
                    CInst::Wrapper(exports) => exports
                        .iter()
                        .map(|(n, cfunc)| match &self.cfuncs[*cfunc] {
                            CFunc::Lifted(idx) => Ok((n.clone(), *idx)),
                            CFunc::Host { .. } => {
                                Err(unsupported("instance export of a host import"))
                            }
                        })
                        .collect::<Result<Vec<_>>>()?,
                    CInst::Imported { .. } => {
                        return Err(unsupported("re-exporting an imported instance"))
                    }
                };
                // Exporting extends the instance index space; the entry is
                // never aliased in the accepted subset, so an empty
                // placeholder keeps indices aligned.
                self.cinstances.push(CInst::Wrapper(Vec::new()));
                self.out.exports.push(ComponentExport {
                    name,
                    item: ExportItem::Instance(exports),
                });
                Ok(())
            }
            other => Err(unsupported(format!("component export of kind {other:?}"))),
        }
    }
}

/// Replace shape-local resource ids (`"<local>#name"`) with ids bound to
/// the importing interface.
fn bind_type(t: &WitType, interface: &str) -> WitType {
    let rec = |t: &WitType| bind_type(t, interface);
    let rebind = |id: &str| match id.strip_prefix("<local>#") {
        Some(name) => format!("{interface}#{name}"),
        None => id.to_string(),
    };
    match t {
        WitType::Own(id) => WitType::Own(rebind(id)),
        WitType::Borrow(id) => WitType::Borrow(rebind(id)),
        WitType::List(t) => WitType::List(Box::new(rec(t))),
        WitType::Option(t) => WitType::Option(Box::new(rec(t))),
        WitType::Record(fs) => {
            WitType::Record(fs.iter().map(|(n, t)| (n.clone(), rec(t))).collect())
        }
        WitType::Tuple(ts) => WitType::Tuple(ts.iter().map(rec).collect()),
        WitType::Variant(cs) => WitType::Variant(
            cs.iter()
                .map(|(n, t)| (n.clone(), t.as_ref().map(&rec)))
                .collect(),
        ),
        WitType::Result { ok, err } => WitType::Result {
            ok: ok.as_ref().map(|t| Box::new(rec(t))),
            err: err.as_ref().map(|t| Box::new(rec(t))),
        },
        other => other.clone(),
    }
}

fn bind_resources(f: &WitFunc, interface: &str) -> WitFunc {
    WitFunc {
        params: f
            .params
            .iter()
            .map(|(n, t)| (n.clone(), bind_type(t, interface)))
            .collect(),
        result: f.result.as_ref().map(|t| bind_type(t, interface)),
    }
}

fn bind_ct(ct: &CT, interface: &str) -> CT {
    match ct {
        CT::Defined(t) => CT::Defined(bind_type(t, interface)),
        CT::Resource(id) => CT::Resource(match id.strip_prefix("<local>#") {
            Some(name) => format!("{interface}#{name}"),
            None => id.clone(),
        }),
        CT::Func(f) => CT::Func(bind_resources(f, interface)),
        other => other.clone(),
    }
}

fn primitive(p: &PrimitiveValType) -> Result<WitType> {
    Ok(match p {
        PrimitiveValType::Bool => WitType::Bool,
        PrimitiveValType::S8 => WitType::S8,
        PrimitiveValType::U8 => WitType::U8,
        PrimitiveValType::S16 => WitType::S16,
        PrimitiveValType::U16 => WitType::U16,
        PrimitiveValType::S32 => WitType::S32,
        PrimitiveValType::U32 => WitType::U32,
        PrimitiveValType::S64 => WitType::S64,
        PrimitiveValType::U64 => WitType::U64,
        PrimitiveValType::F32 => WitType::F32,
        PrimitiveValType::F64 => WitType::F64,
        PrimitiveValType::Char => WitType::Char,
        PrimitiveValType::String => WitType::String,
        other => return Err(unsupported(format!("primitive type {other:?}"))),
    })
}

/// Parse a nested component, accepting only the wrapper shape
/// wit-component emits: function imports re-exported as function exports.
fn parse_nested_component(bytes: &[u8]) -> Result<NestedComponent> {
    let mut nested = NestedComponent::default();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::Version { .. } | Payload::CustomSection(_) | Payload::End(_) => {}
            Payload::ComponentTypeSection(_) => {}
            Payload::ComponentImportSection(reader) => {
                for import in reader {
                    let import = import?;
                    match import.ty {
                        ComponentTypeRef::Func(_) => {
                            nested.func_imports.push(import.name.name.to_string());
                        }
                        other => {
                            return Err(unsupported(format!(
                                "nested component import of kind {other:?}"
                            )));
                        }
                    }
                }
            }
            Payload::ComponentExportSection(reader) => {
                for export in reader {
                    let export = export?;
                    match export.kind {
                        ComponentExternalKind::Func => {
                            let idx = export.index as usize;
                            if idx >= nested.func_imports.len() + nested.func_exports.len() {
                                return Err(unsupported("nested export out of range"));
                            }
                            // Func index space = imports ++ exports-so-far;
                            // both resolve to an import in the wrapper shape.
                            let inner = if idx < nested.func_imports.len() {
                                idx
                            } else {
                                nested.func_exports[idx - nested.func_imports.len()].1
                            };
                            nested
                                .func_exports
                                .push((export.name.name.to_string(), inner));
                        }
                        other => {
                            return Err(unsupported(format!(
                                "nested component export of kind {other:?}"
                            )));
                        }
                    }
                }
            }
            other => {
                return Err(unsupported(format!("nested component payload {other:?}")));
            }
        }
    }
    Ok(nested)
}

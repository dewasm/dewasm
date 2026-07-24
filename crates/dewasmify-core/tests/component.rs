//! Component parsing tests over committed `.wat` component fixtures
//! (the `wat` crate encodes component text, so no binary artifacts are
//! needed).

use dewasmify_core::component::{
    build_component, is_component, CoreInstance, CoreItem, ExportItem, WitType,
};
use dewasmify_core::feature::UnsupportedError;

fn parse(text: &str) -> dewasmify_core::component::Component {
    let bytes = wat::parse_str(text).expect("component text parses");
    assert!(is_component(&bytes), "layer field is 1");
    build_component(&bytes).expect("component builds")
}

#[test]
fn minimal_lift() {
    let c = parse(
        r#"
        (component
          (core module $m
            (func (export "add") (param i32 i32) (result i32)
              (i32.add (local.get 0) (local.get 1)))
          )
          (core instance $i (instantiate $m))
          (func (export "add") (param "a" u32) (param "b" u32) (result u32)
            (canon lift (core func $i "add"))
          )
        )
        "#,
    );
    assert_eq!(c.core_modules.len(), 1);
    assert_eq!(c.imports.len(), 0);
    assert_eq!(c.instances.len(), 1);
    assert!(matches!(
        c.instances[0],
        CoreInstance::Instantiate { module: 0, ref args } if args.is_empty()
    ));
    assert_eq!(c.lifted.len(), 1);
    assert_eq!(c.lifted[0].ty.params.len(), 2);
    assert_eq!(c.lifted[0].ty.params[0].1, WitType::U32);
    assert_eq!(c.lifted[0].ty.result, Some(WitType::U32));
    assert_eq!(c.exports.len(), 1);
    assert_eq!(c.exports[0].name, "add");
    assert!(matches!(c.exports[0].item, ExportItem::Func(0)));
}

#[test]
fn import_lower_with_memory_and_string() {
    let c = parse(
        r#"
        (component
          (type $log_ty (func (param "msg" string)))
          (import "example:host/log@0.2.0" (instance $host
            (export "log" (func (type $log_ty)))
          ))
          (alias export $host "log" (func $log))
          (core module $mem_mod
            (memory (export "memory") 1)
            (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
              (i32.const 0))
          )
          (core instance $mem_i (instantiate $mem_mod))
          (alias core export $mem_i "memory" (core memory $mem))
          (alias core export $mem_i "cabi_realloc" (core func $realloc))
          (core func $lowered (canon lower (func $log)
            (memory $mem) (realloc $realloc) string-encoding=utf8))
          (core module $m
            (import "example:host/log@0.2.0" "log" (func $log (param i32 i32)))
            (func (export "run") (call $log (i32.const 8) (i32.const 5)))
          )
          (core instance $mi (instantiate $m
            (with "example:host/log@0.2.0" (instance
              (export "log" (func $lowered))))))
          (func (export "run") (canon lift (core func $mi "run")))
        )
        "#,
    );
    assert_eq!(c.imports.len(), 1);
    assert_eq!(c.imports[0].name, "example:host/log@0.2.0");
    assert_eq!(c.imports[0].funcs.len(), 1);
    assert_eq!(c.imports[0].funcs[0].0, "log");
    assert_eq!(c.imports[0].funcs[0].1.params[0].1, WitType::String);
    // The lowering resolved its memory/realloc to the first instance.
    let CoreInstance::Synthetic(items) = &c.instances[1] else {
        panic!("expected synthetic instance feeding $m");
    };
    let CoreItem::Lower { host, opts } = &items[0].1 else {
        panic!("expected lowered adapter");
    };
    assert_eq!(host.func, "log");
    assert_eq!(opts.memory, Some((0, "memory".to_string())));
    assert_eq!(opts.realloc, Some((0, "cabi_realloc".to_string())));
}

#[test]
fn rejects_unknown_layer0_input() {
    let bytes = wat::parse_str("(module)").expect("module");
    assert!(!is_component(&bytes));
}

#[test]
fn rejects_out_of_subset_with_attribution() {
    // A value import is outside the accepted subset.
    let bytes = wat::parse_str(
        r#"
        (component
          (core module $m)
          (core instance (instantiate $m))
          (component $inner
            (component $innermost)
            (instance (export "x") (instantiate $innermost))
          )
        )
        "#,
    )
    .expect("component text parses");
    let err = build_component(&bytes).expect_err("nested non-wrapper component rejected");
    let unsupported = err
        .chain()
        .find_map(|e| e.downcast_ref::<UnsupportedError>())
        .expect("attributed refusal");
    assert_eq!(unsupported.features[0].id(), "component-model");
}

#[test]
fn shim_table_pattern() {
    // The wit-component indirect-lowering shape: a shim module exporting a
    // funcref table, fixed up after the main module provides memory.
    let c = parse(
        r#"
        (component
          (type $get_ty (func (result u32)))
          (import "example:host/env@0.2.0" (instance $host
            (export "get" (func (type $get_ty)))
          ))
          (core module $shim
            (table (export "$imports") 1 1 funcref)
            (func (export "0") (result i32)
              (call_indirect (result i32) (i32.const 0)))
          )
          (core instance $shim_i (instantiate $shim))
          (core module $main
            (import "example:host/env@0.2.0" "get" (func $get (result i32)))
            (memory (export "memory") 1)
            (func (export "run") (result i32) (call $get))
          )
          (alias core export $shim_i "0" (core func $get_shim))
          (core instance $main_i (instantiate $main
            (with "example:host/env@0.2.0" (instance
              (export "get" (func $get_shim))))))
          (alias export $host "get" (func $get_host))
          (core func $get_lowered (canon lower (func $get_host)))
          (core module $fixups
            (import "" "0" (func $f (result i32)))
            (import "" "$imports" (table $t 1 1 funcref))
            (elem (table $t) (i32.const 0) funcref (ref.func $f))
          )
          (alias core export $shim_i "$imports" (core table $imports))
          (core instance (instantiate $fixups
            (with "" (instance
              (export "0" (func $get_lowered))
              (export "$imports" (table $imports))))))
          (func (export "run") (result u32)
            (canon lift (core func $main_i "run")))
        )
        "#,
    );
    assert_eq!(c.core_modules.len(), 3);
    assert_eq!(c.instances.len(), 5, "3 instantiations + 2 synthetic");
    // The fixups instantiation is fed a synthetic instance carrying the
    // lowered adapter and the shim's table.
    let synthetic = c
        .instances
        .iter()
        .filter_map(|i| match i {
            CoreInstance::Synthetic(items) => Some(items),
            _ => None,
        })
        .next_back()
        .expect("synthetic instance");
    assert!(synthetic
        .iter()
        .any(|(n, i)| n == "0" && matches!(i, CoreItem::Lower { .. })));
    assert!(synthetic.iter().any(|(n, i)| n == "$imports"
        && matches!(i, CoreItem::InstanceExport { instance: 0, name } if name == "$imports")));
}

#[test]
fn synthesize_adapters() {
    let c = parse(
        r#"
        (component
          (type $log_ty (func (param "msg" string) (result u32)))
          (import "example:host/log@0.2.0" (instance $host
            (export "log" (func (type $log_ty)))
          ))
          (alias export $host "log" (func $log))
          (core module $mem_mod
            (memory (export "memory") 1)
            (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
              (i32.const 0))
          )
          (core instance $mem_i (instantiate $mem_mod))
          (alias core export $mem_i "memory" (core memory $mem))
          (alias core export $mem_i "cabi_realloc" (core func $realloc))
          (core func $lowered (canon lower (func $log)
            (memory $mem) (realloc $realloc) string-encoding=utf8))
          (core module $m
            (import "example:host/log@0.2.0" "log" (func $log (param i32 i32) (result i32)))
            (func (export "run") (result i32) (call $log (i32.const 8) (i32.const 5)))
          )
          (core instance $mi (instantiate $m
            (with "example:host/log@0.2.0" (instance
              (export "log" (func $lowered))))))
          (func (export "run") (result u32) (canon lift (core func $mi "run")))
        )
        "#,
    );
    let synth = dewasmify_core::canon::synthesize(&c).expect("synthesis");
    // One lower adapter: (i32 ptr, i32 len) -> i32, lifting the string and
    // calling host.log.
    assert_eq!(synth.lowers.module.funcs.len(), 1);
    let lower_ty = &synth.lowers.module.types[synth.lowers.module.funcs[0].type_idx as usize];
    assert_eq!(lower_ty.params.len(), 2);
    assert_eq!(lower_ty.results.len(), 1);
    assert!(synth
        .lowers
        .module
        .imported_funcs
        .iter()
        .any(|f| f.module == "host" && f.name == "example:host/log@0.2.0#log"));
    // One lift adapter: host value in, calls core.2:run.
    assert_eq!(synth.lifts.module.funcs.len(), 1);
    assert!(synth
        .lifts
        .module
        .imported_funcs
        .iter()
        .any(|f| f.module == "core" && f.name == "2:run"));
}

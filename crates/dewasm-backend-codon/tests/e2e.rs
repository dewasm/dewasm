//! Codon end-to-end suites: the `BackendUnderTest` impl, the named glue constants, and the macro invocations declaring which shared cases this backend runs.
//!
//! Not yet invoked (the WASI surface is the minimal microbenchmark set until full WASI p1 lands, issue #310 notes the split):
//! - `wasi_suite!`/`standalone_dir_e2e!`/`wasi_root_containment_e2e!` and the filesystem/C-API app cases: they exercise WASI syscalls (fd table, filesystem, clock, poll) the bundled runtime does not implement yet.
//! - `wasi_import_override_e2e!`/`custom_wasi_provider_e2e!`/`partial_override_e2e!`/`stdio_capture_e2e!`: same WASI surface, plus a capture-capable fd 1 (the bundled fd_write writes straight to the process fd).
//! - `cowsay_*` and the other app execution cases: converting works (the convert suite covers it), but the giant-artifact `codon build -release` cost is unsettled (minutes for cowsay, see issue #310).
//! - `qjs_repl_pty_e2e!`/`deep_recursion_e2e!`: the standalone entrypoint runs on the native stack with no depth mitigation yet.

use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::{Backend, RuntimeLinkage};
use dewasm_backend_codon::{find_codon, CodonBackend};
use dewasm_test_helper::BackendUnderTest;

mod common;

struct Codon;

impl BackendUnderTest for Codon {
    fn name(&self) -> &'static str {
        "codon"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &CodonBackend
    }

    /// Compile `source` to the crate's shared cache binary and run it with the Codon runtime dylibs on the loader path.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        match common::build_codon(source) {
            Err(build) => build,
            Ok(bin) => common::run_codon_binary(&bin, args, stdin),
        }
    }

    fn compose_modules(
        &self,
        dir: &Path,
        modules: &[(&str, &str)],
        shared_runtime: bool,
    ) -> String {
        let mut imports = Vec::new();
        if shared_runtime {
            let mut units = std::collections::BTreeSet::new();
            let mut classes = Vec::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_codon::generate_class_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Alias("Rt".to_string()),
                    false,
                )
                .expect("generate");
                units.extend(u);
                classes.push((name.to_lowercase(), *name, src));
            }
            std::fs::write(
                dir.join("rt.codon"),
                dewasm_backend_codon::shared_runtime(&units).expect("bundle runtime"),
            )
            .unwrap();
            imports.push("from rt import Rt".to_string());
            for (stem, _name, src) in classes {
                std::fs::write(
                    dir.join(format!("{stem}.codon")),
                    format!("from rt import Rt\n\n{src}"),
                )
                .unwrap();
                // The wrapper classes the boxed exports reference live next to the generated class; a star import brings them along with it.
                imports.push(format!("from {stem} import *"));
            }
        } else {
            for (wat, name) in modules {
                let stem = name.to_lowercase();
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, _) = dewasm_backend_codon::generate_class_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Embedded,
                    false,
                )
                .expect("generate");
                std::fs::write(dir.join(format!("{stem}.codon")), src).unwrap();
                imports.push(format!("from {stem} import *"));
            }
        }
        imports.join("\n")
    }

    /// `codon run` executes in-process, so no dylib path or prebuilt binary is needed; the working directory holds the module files the driver imports.
    fn run_in_dir(&self, dir: &Path, driver: &str) -> Output {
        let path = dir.join("driver.codon");
        std::fs::write(&path, driver).unwrap();
        let codon = find_codon()
            .expect("codon toolchain not found on PATH (or $DEWASM_CODON): see docs/testing.md");
        dewasm_test_helper::run_command_bytes(
            Command::new(codon)
                .arg("run")
                .arg("-release")
                .arg(&path)
                .current_dir(dir),
            b"",
        )
    }
}

/// Driver for the plain-arithmetic library case, through the boxed export surface an embedder uses.
const CODON_LIBRARY_ADD_GLUE: &str = r#"_i = Add(Dict[str, Dict[str, AddRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
print(_i.exports["add"].fn.invoke([AddRt.Val.of_i32(UInt[32](2)), AddRt.Val.of_i32(UInt[32](3))])[0].i32())
print(_i.exports["add"].fn.invoke([AddRt.Val.of_i32(UInt[32](4294967295)), AddRt.Val.of_i32(UInt[32](1))])[0].i32())
print(_i.exports["fib"].fn.invoke([AddRt.Val.of_i32(UInt[32](10))])[0].i32())
"#;

/// Driver for the shared-table case: instantiate the exporter and the importer linked against it, then print `call0` (call_indirect through the shared table -> 42).
const CODON_SHARED_TABLE_GLUE: &str = r#"_a = TableExp(Dict[str, Dict[str, Rt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_b = TableImp({"a": _a.exports}, List[str](), Dict[str, str](), Dict[str, str]())
print(_b.exports["call0"].fn.invoke(List[Rt.Val]())[0].i32())
"#;

/// Driver for the embedded-coexistence case: two independent Embedded artifacts in one namespace.
/// Each carries its own runtime class (`AlphaRt`/`BetaRt`), so their trap types are distinct: Beta's except arm must not catch Alpha's trap.
const CODON_EMBEDDED_COEXIST_GLUE: &str = r#"_a = Alpha(Dict[str, Dict[str, AlphaRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_b = Beta(Dict[str, Dict[str, BetaRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
print(_a.exports["div"].fn.invoke([AlphaRt.Val.of_i32(UInt[32](7)), AlphaRt.Val.of_i32(UInt[32](2))])[0].i32())
print(_b.exports["div"].fn.invoke([BetaRt.Val.of_i32(UInt[32](4294967289)), BetaRt.Val.of_i32(UInt[32](2))])[0].i32())
_distinct = True
try:
    _a.exports["div"].fn.invoke([AlphaRt.Val.of_i32(UInt[32](1)), AlphaRt.Val.of_i32(UInt[32](0))])
except BetaRt.Trap:
    _distinct = False
except AlphaRt.Trap:
    pass
print("distinct-rt" if _distinct else "same-rt")
try:
    _a.exports["div"].fn.invoke([AlphaRt.Val.of_i32(UInt[32](1)), AlphaRt.Val.of_i32(UInt[32](0))])
except AlphaRt.Trap:
    print("trapped")
"#;

dewasm_test_helper::library_add_e2e!(Codon, CODON_LIBRARY_ADD_GLUE);
dewasm_test_helper::shared_table_e2e!(Codon, CODON_SHARED_TABLE_GLUE);
dewasm_test_helper::embedded_coexist_e2e!(Codon, CODON_EMBEDDED_COEXIST_GLUE);

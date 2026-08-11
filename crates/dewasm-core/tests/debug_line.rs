//! Core coverage for DWARF `.debug_line` source back-mapping: the `BuildOptions::debug_line` opt-in produces [`ir::Stmt::SourceLine`] markers resolved through the interned [`ir::Module::debug_files`], and the default build stays byte-for-byte marker-free.
//!
//! The fixture (`examples/apps/src/dwarf_fixture.c`, built by setup.sh into the cache with `-g -O1`) pins the one calibration constant this feature has: the DWARF address base. `add_mul` is a folded, single-statement function whose only marker must land on the exact source line of its first statement; a wrong base shifts that line (or drops the marker entirely), so this test fails loudly for any miscalibration.

use std::path::{Path, PathBuf};

use dewasm_core::ir::{ExportKind, SourcePos, Stmt};
use dewasm_core::{build_module_with_options, BuildOptions};

/// Cached DWARF fixture; a missing cache fails loud with a setup instruction, never skips.
fn fixture_bytes() -> Vec<u8> {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache/dwarf-fixture.wasm");
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "dwarf-fixture not cached ({}, {e}): run examples/apps/setup.sh (see docs/testing.md)",
            path.display()
        )
    })
}

/// The `[Stmt::SourceLine]` positions in the body of the exported function named `export`, as `(file_path, line, col)`.
fn export_source_positions<'m>(
    module: &'m dewasm_core::ir::Module,
    export: &str,
) -> Vec<(&'m str, u32, u32)> {
    let n_imported = module.imported_funcs.len() as u32;
    let idx = module
        .exports
        .iter()
        .find_map(|e| match e.kind {
            ExportKind::Func(idx) if e.name == export => Some(idx),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture has no exported function `{export}`"));
    assert!(idx >= n_imported, "`{export}` is an imported function");
    let func = &module.funcs[(idx - n_imported) as usize];
    func.body
        .iter()
        .filter_map(|s| match s {
            Stmt::SourceLine(SourcePos { file, line, col }) => {
                Some((module.debug_files[*file as usize].as_str(), *line, *col))
            }
            _ => None,
        })
        .collect()
}

/// Any `SourceLine` marker anywhere in the module (nested statements included).
fn has_any_source_line(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::SourceLine(_) => true,
        Stmt::Block { body, .. } | Stmt::Loop { body, .. } => has_any_source_line(body),
        Stmt::If { then, els, .. } => has_any_source_line(then) || has_any_source_line(els),
        _ => false,
    })
}

#[test]
fn debug_line_disabled_by_default_is_marker_free() {
    let bytes = fixture_bytes();
    let module = build_module_with_options(&bytes, &BuildOptions::default()).unwrap();
    assert!(
        module.debug_files.is_empty(),
        "default build must not populate debug_files"
    );
    for func in &module.funcs {
        assert!(
            !has_any_source_line(&func.body),
            "default build must emit no SourceLine markers"
        );
    }
}

#[test]
fn debug_line_interns_fixture_source_file() {
    let bytes = fixture_bytes();
    let module = build_module_with_options(&bytes, &BuildOptions { debug_line: true }).unwrap();
    assert!(
        module
            .debug_files
            .iter()
            .any(|f| f.ends_with("dwarf_fixture.c")),
        "debug_files should intern the fixture source, got {:?}",
        module.debug_files
    );
}

#[test]
fn debug_line_calibration_pins_add_mul_first_statement() {
    let bytes = fixture_bytes();
    let module = build_module_with_options(&bytes, &BuildOptions { debug_line: true }).unwrap();

    // `add_mul` folds to a single `Return`, so its lone marker is the one the fallthrough-return path emits, and it must resolve to the exact source line of `int product = a * b;` (line 22 of dwarf_fixture.c). This is the address-base calibration: a wrong base moves this line or yields none.
    let positions = export_source_positions(&module, "add_mul");
    let (file, line, _col) = *positions
        .first()
        .expect("add_mul must carry at least one source marker");
    assert!(
        file.ends_with("dwarf_fixture.c"),
        "add_mul marker should point into the fixture source, got {file}"
    );
    assert_eq!(
        line, 22,
        "add_mul's first statement is line 22 (int product = a * b;); \
         a different line means the DWARF address base is miscalibrated"
    );

    // `sum_prefix` spans several source lines, so it must yield more than one change-point marker, all inside the fixture source.
    let sp = export_source_positions(&module, "sum_prefix");
    assert!(
        sp.len() >= 2,
        "sum_prefix should produce multiple change-point markers, got {sp:?}"
    );
    assert!(
        sp.iter().all(|(f, _, _)| f.ends_with("dwarf_fixture.c")),
        "all sum_prefix markers should point into the fixture source, got {sp:?}"
    );
}

//! `go vet` over converted artifacts.
//! A Go project that imports an artifact runs its own vet over it, so generated source has to satisfy the same checks hand-written Go does; before issue #214 a converted mruby carried about 7,200 diagnostics, all of them unreachable statements and self-assignments.
//!
//! The cases are the shapes the emitter has to get right: a standalone program (frame exits and `br_table` switches), a standalone program using exception handling (`try_table` closures and catch handlers), a fixture whose pruned statements carry the last use of a label and a local, and a library artifact (the package layout an embedder imports).

use std::process::Command;

use dewasm_backend::Mode;
use dewasm_backend_go::{find_go, GoBackend};

mod common;

/// Convert the cached app `name` in standalone mode.
fn standalone_app(name: &str) -> String {
    let wasm_path = dewasm_test_helper::apps_cache_dir().join(format!("{name}.wasm"));
    assert!(
        wasm_path.exists(),
        "{name} not cached: run examples/apps/setup.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    dewasm_test_helper::convert_on_big_stack(&GoBackend, &bytes, Mode::Standalone, name)
}

/// Run `go vet` over `source` in a throwaway module laid out the way `common::build_go` lays one out for `go build`: `package main` is a file beside the `go.mod`, a library package a directory of its own name.
/// A missing `go` toolchain is a loud failure, as everywhere else in this crate's tests.
fn assert_vet_clean(case: &str, source: &str) {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO): see docs/testing.md");
    let dir = dewasm_test_helper::fresh_scratch_dir(&format!("go-vet-{case}"));
    std::fs::write(dir.join("go.mod"), "module dewasmvet\n\ngo 1.21\n").unwrap();
    let package = common::package_clause(source);
    if package == "main" {
        std::fs::write(dir.join("main.go"), source).unwrap();
    } else {
        let pkg_dir = dir.join(package);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join(format!("{package}.go")), source).unwrap();
    }

    let out = Command::new(&go)
        .args(["vet", "./..."])
        .current_dir(&dir)
        // A stray `go.work` above the temp dir would otherwise pull this throwaway module into a workspace that does not list it.
        .env("GOWORK", "off")
        .output()
        .expect("spawn go vet");
    assert!(
        out.status.success(),
        "go vet rejected the {case} artifact:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cowsay_standalone_artifact_is_vet_clean() {
    assert_vet_clean("cowsay", &standalone_app("cowsay"));
}

/// mruby is the exception-handling app (its setjmp/longjmp lowering uses `try_table`/`throw`), and the artifact issue #214 measured.
/// No speed token: this file measured 5.0 s against a cold `go` build cache and 0.8 s against a warm one, cowsay-class like the `mruby_eh` e2e case.
#[test]
fn mruby_standalone_artifact_is_vet_clean() {
    assert_vet_clean("mruby", &standalone_app("mruby"));
}

/// A loop with no exit makes the statements after it unreachable, and dropping those can take the last branch to the enclosing frame's label and the last read of a local with them: Go rejects both an unused label and an unused variable, so the emitter has to drop the label and blank the local too.
/// No cached app produces this shape (every one of them keeps its label count across the change), so it is pinned here.
const PRUNED_TAIL_WAT: &str = r#"(module
  (func $sink (param i32))
  (func (export "loop_without_exit") (result i32)
    (local i32)
    (local.set 0 (i32.const 5))
    (block $b
      (loop $l (br $l))
      (call $sink (local.get 0))
      (br $b))
    (i32.const 1))
  (func (export "if_arms_both_branch") (param i32) (result i32)
    (block $b
      (if (local.get 0) (then (br $b)) (else (br $b))))
    (i32.const 2))
  (func (export "br_table_tail") (param i32) (result i32)
    (block $a (block $b (br_table $a $b (local.get 0))))
    (i32.const 3)))
"#;

#[test]
fn a_pruned_tail_takes_its_label_and_local_reads_with_it() {
    let bytes = wat::parse_str(PRUNED_TAIL_WAT).expect("parse wat");
    let src = dewasm_test_helper::convert_bytes(&GoBackend, &bytes, Mode::Library, "pruned");
    assert_vet_clean("pruned", &src);
}

#[test]
fn library_artifact_is_vet_clean() {
    let src = dewasm_test_helper::convert(
        &GoBackend,
        &dewasm_test_helper::examples_dir().join("add.wat"),
        Mode::Library,
        "adder",
    );
    assert_vet_clean("adder", &src);
}

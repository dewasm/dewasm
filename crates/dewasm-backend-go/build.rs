//! Embeds the runtime units from runtime/go/units/ as `UNIT_SOURCES: &[(&str, &str)]` (unit id, source). Mirrors the Python crate's build.rs; only the source directory and extension differ.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let units_dir = Path::new(&manifest_dir).join("../../runtime/go/units");
    let units_dir = units_dir.canonicalize().expect("runtime/go/units exists");
    println!("cargo:rerun-if-changed={}", units_dir.display());

    let mut entries = Vec::new();
    for scope in std::fs::read_dir(&units_dir).unwrap() {
        let scope = scope.unwrap();
        if !scope.file_type().unwrap().is_dir() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", scope.path().display());
        for file in std::fs::read_dir(scope.path()).unwrap() {
            let file = file.unwrap();
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("go") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let id = format!(
                "{}/{}",
                scope.file_name().to_str().unwrap(),
                path.file_stem().unwrap().to_str().unwrap()
            );
            entries.push((id, path));
        }
    }
    entries.sort();

    let mut out = String::from("pub static UNIT_SOURCES: &[(&str, &str)] = &[\n");
    for (id, path) in entries {
        let _ = writeln!(out, "    ({id:?}, include_str!({:?})),", path.display());
    }
    out.push_str("];\n");

    let out_path = Path::new(&std::env::var("OUT_DIR").unwrap()).join("units.rs");
    std::fs::write(out_path, out).unwrap();
}

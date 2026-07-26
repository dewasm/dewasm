//! Lint for the runtime units: every reference a unit body makes to another
//! unit must be declared in its `// requires:` header. Mirrors the Go backend's
//! units lint (ADR-6), adjusted for Java: `Rt.<name>` helper calls,
//! `memory.<name>` memory-method calls, and per-scope sibling calls (a bare
//! `name(` not preceded by a `.`). A second test compiles the whole bundle with
//! `javac`, so a syntax error in any unit — not just the subset cowsay uses — is
//! caught (ADR-15: a missing toolchain fails loud).

use std::collections::BTreeSet;

use dewasm_backend_java::{bundler, find_javac, full_bundle_java};
use regex::Regex;

#[test]
fn all_units_bundle() {
    bundler().bundle_all(0).expect("full bundle resolves");
}

#[test]
fn declared_requires_cover_references() {
    let b = bundler();
    let unit_ids: BTreeSet<&str> = b.units().map(|u| u.id.as_str()).collect();

    let rt_call = Regex::new(r"Rt\.([a-z_][a-z0-9_]*)").unwrap();
    let memory_call = Regex::new(r"\bmemory\.([a-z_][a-z0-9_]*)").unwrap();
    // One sibling-call matcher per scoped unit: a bare `name(` not preceded by a
    // `.` (so `memory.init(` is not read as a call to the sibling `init`).
    let sibling_calls: Vec<(&str, Regex)> = unit_ids
        .iter()
        .filter_map(|id| {
            let name = id.split('/').nth(1).unwrap();
            if name.starts_with('_') {
                return None;
            }
            let re = Regex::new(&format!(r"(^|[^\w.]){}\s*\(", regex::escape(name))).unwrap();
            Some((*id, re))
        })
        .collect();

    let mut problems = Vec::new();
    for unit in b.units() {
        let scope = unit.id.split('/').next().unwrap();
        let declared: BTreeSet<&str> = unit.requires.iter().map(|s| s.as_str()).collect();
        let mut demand = |dep: String, what: &str| {
            if dep == unit.id || declared.contains(dep.as_str()) {
                return;
            }
            // Scope preludes and the root prelude are implicit.
            if dep.ends_with("/_class") || dep.ends_with("/_prelude") {
                return;
            }
            problems.push(format!(
                "{}: uses {what} but does not require {dep}",
                unit.id
            ));
        };

        // Strip `//` comment lines so requires headers/comments don't count.
        let code: String = unit
            .body
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        for cap in rt_call.captures_iter(&code) {
            demand(format!("rt/{}", &cap[1]), &format!("Rt.{}", &cap[1]));
        }
        for cap in memory_call.captures_iter(&code) {
            demand(
                format!("memory/{}", &cap[1]),
                &format!("memory.{}", &cap[1]),
            );
        }
        for (sibling, re) in &sibling_calls {
            // Sibling calls are only in-scope (no receiver prefix).
            let Some(name) = sibling.strip_prefix(&format!("{scope}/")) else {
                continue;
            };
            if *sibling == unit.id {
                continue;
            }
            if re.is_match(&code) {
                demand(sibling.to_string(), &format!("{name}(...)"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "unit dependency drift:\n{}",
        problems.join("\n")
    );
}

/// The whole runtime — every unit, not just the subset cowsay uses — must be
/// valid Java. Compile the full bundle with `javac` (ADR-15).
#[test]
fn all_units_compile_as_java() {
    let javac =
        find_javac().expect("javac not found on PATH (or $DEWASM_JAVAC) — see docs/testing.md");
    let source = full_bundle_java().expect("full bundle assembles");
    let dir = std::env::temp_dir().join(format!("dewasm-java-units-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("Main.java");
    std::fs::write(&src, &source).unwrap();
    let out = std::process::Command::new(&javac)
        .arg("-d")
        .arg(&dir)
        .arg(&src)
        .output()
        .expect("spawn javac");
    assert!(
        out.status.success(),
        "full runtime bundle failed to compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

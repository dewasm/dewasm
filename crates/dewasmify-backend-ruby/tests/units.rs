//! Lint for the runtime units: every reference a unit body makes to
//! another unit must be declared in its `# requires:` header. This is the
//! static half of the drift defence; the dynamic half is the spec harness
//! running against minimal bundles (ADR-6).

use std::collections::BTreeSet;

use dewasmify_backend_ruby::bundler;
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
    let rt_const = Regex::new(r"Rt::([A-Z]\w*)").unwrap();
    let memory_call = Regex::new(r"@memory\.([a-z_][a-z0-9_]*)").unwrap();
    // One precompiled bare-call matcher per unit name.
    let bare_calls: Vec<(&str, Regex)> = unit_ids
        .iter()
        .map(|id| {
            let name = id.split('/').nth(1).unwrap();
            let re = Regex::new(&format!(r#"(^|[^\w.@:"]){}\("#, regex::escape(name))).unwrap();
            (*id, re)
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
            if dep.ends_with("/_class") || dep.ends_with("/_module") {
                return;
            }
            problems.push(format!("{}: uses {what} but does not require {dep}", unit.id));
        };

        let code: String = unit
            .body
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");

        for cap in rt_call.captures_iter(&code) {
            demand(format!("rt/{}", &cap[1]), &format!("Rt.{}", &cap[1]));
        }
        for cap in rt_const.captures_iter(&code) {
            let dep = match &cap[1] {
                "Trap" => "rt/trap".to_string(),
                "Exit" => "rt/exit".to_string(),
                "M32" | "M64" => continue, // root prelude, always bundled
                "Memory" => "memory/_class".to_string(),
                "Table" => "table/_class".to_string(),
                "WASI" => "wasi/_class".to_string(),
                other => panic!("{}: unknown runtime constant Rt::{other}", unit.id),
            };
            demand(dep, &format!("Rt::{}", &cap[1]));
        }
        for cap in memory_call.captures_iter(&code) {
            demand(format!("memory/{}", &cap[1]), &format!("@memory.{}", &cap[1]));
        }

        // Bare sibling calls within the same scope (with parentheses; a
        // parenless bare call cannot be told apart from a local variable,
        // so those cases must keep their requires by hand).
        for (sibling, bare) in &bare_calls {
            let Some(name) = sibling.strip_prefix(&format!("{scope}/")) else {
                continue;
            };
            if name.starts_with('_') || *sibling == unit.id {
                continue;
            }
            if bare.is_match(&code) {
                demand(sibling.to_string(), &format!("{name}(...)"));
            }
        }
    }
    assert!(problems.is_empty(), "unit dependency drift:\n{}", problems.join("\n"));
}

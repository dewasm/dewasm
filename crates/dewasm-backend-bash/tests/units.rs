//! Lint for the bash runtime units: every reference a unit body makes to
//! another unit must be declared in its `# requires:` header, and every
//! unit function must end with an explicit `return`/`:` — a trailing
//! arithmetic statement would leak status 1 into the `|| return $?` trap
//! cascade (ADR-11). Static half of the ADR-6 drift defence.

use std::collections::BTreeSet;

use dewasm_backend_bash::bundler;
use regex::Regex;

#[test]
fn all_units_bundle() {
    bundler().bundle_all(0).expect("full bundle resolves");
}

#[test]
fn declared_requires_cover_references() {
    let b = bundler();
    let unit_ids: BTreeSet<&str> = b.units().map(|u| u.id.as_str()).collect();
    let call = Regex::new(r"\b(rt|mem|wasi)_([a-z0-9_]+)").unwrap();

    let mut problems = Vec::new();
    for unit in b.units() {
        let declared: BTreeSet<&str> = unit.requires.iter().map(|s| s.as_str()).collect();
        let code: String = unit
            .body
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for cap in call.captures_iter(&code) {
            let dep = format!("{}/{}", &cap[1], &cap[2]);
            if dep == unit.id || declared.contains(dep.as_str()) {
                continue;
            }
            if !unit_ids.contains(dep.as_str()) {
                // Longest-match artifacts (e.g. rt_i32 inside rt_i32_clz)
                // cannot occur because the regex is greedy; anything else
                // unknown is a genuine typo.
                problems.push(format!("{}: references unknown unit {dep}", unit.id));
                continue;
            }
            problems.push(format!(
                "{}: uses {}_{} but does not require {dep}",
                unit.id, &cap[1], &cap[2]
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "unit dependency drift:\n{}",
        problems.join("\n")
    );
}

#[test]
fn unit_functions_end_with_return() {
    let b = bundler();
    let mut problems = Vec::new();
    for unit in b.units() {
        let lines: Vec<&str> = unit.body.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if *line != "}" {
                continue;
            }
            let last = lines[..i]
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(|l| l.trim())
                .unwrap_or("");
            if !(last.starts_with("return") || last == ":" || last.ends_with("return $?; }")) {
                problems.push(format!(
                    "{}: function body ends with `{last}` instead of an explicit return",
                    unit.id
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "unit functions must end with an explicit return (status discipline):\n{}",
        problems.join("\n")
    );
}

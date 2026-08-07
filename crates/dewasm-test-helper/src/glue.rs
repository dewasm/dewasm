//! The placeholder-substitution helper for direct-string test glue: per-language test glue is written as a named `&str` constant in each backend crate, never a resolver function or a `(name, glue)` table — everything static (class name, argv, env, preopen guest paths) is written out literally. Only runtime-computed values travel as `{name}` placeholders — `{scratch}`, `{cache}`, `{host}`, `{guest}` — which [`fill`] substitutes; no escaping is needed since those values are plain temp/cache directory paths.

/// Substitute each `(name, value)`'s `{name}` placeholder in `template` with `value`; a placeholder absent from `template` is a no-op.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

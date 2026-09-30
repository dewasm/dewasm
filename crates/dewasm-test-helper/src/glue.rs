//! The placeholder-substitution helper for direct-string test glue.
//! Per-language test glue is written as a named `&str` constant in each backend crate.
//! It is never a resolver function or a `(name, glue)` table.
//! Everything static (class name, argv, env, preopen guest paths) is written out literally.
//! Only runtime-computed values travel as `{name}` placeholders, which [`fill`] substitutes.
//! Those are `{scratch}`, `{cache}`, `{host}`, and `{guest}`.
//! No escaping is needed, since those values are plain temp/cache directory paths.

/// Substitute each `(name, value)`'s `{name}` placeholder in `template` with `value`.
/// A placeholder absent from `template` is a no-op.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

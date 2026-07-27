//! The placeholder-substitution helper for direct-string test glue (ADR-27
//! revision).
//!
//! Per-language test glue — the host-language driver appended after a converted
//! module to instantiate and exercise it — is written as a named `&str`
//! constant in each backend crate and passed to a per-case macro as a plain
//! argument (never a resolver function, a `match` on the case name, or a
//! `(name, glue)` pair list). Everything a case needs statically — the class
//! name, argv, env, preopen *guest* paths — is written out literally in the
//! glue itself.
//!
//! # Placeholders
//!
//! The one thing that cannot be a literal is a value the test runner computes
//! at run time: the host path of a fresh scratch directory, the app-cache
//! directory, or (for the WASI filesystem template) the preopen host/guest
//! pair. Those are handled by a placeholder convention: [`fill`] substitutes
//! `{name}` tokens with runtime-computed values. Each runner documents which
//! placeholders it supplies; the full set in use is `{scratch}`, `{cache}`,
//! `{host}`, and `{guest}`.
//!
//! No escaping is performed or needed: glue authors control the surrounding
//! string, and the substituted values are plain temporary/cache directory paths
//! with no characters that would need quoting in the host languages.

/// Substitute `{name}` placeholders in `template` with the given values, a plain
/// literal replace of `{name}` for each `(name, value)` pair (no escaping — see
/// the module docs). A placeholder absent from `template` is simply a no-op, so
/// a runner can offer more placeholders than a given glue uses.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

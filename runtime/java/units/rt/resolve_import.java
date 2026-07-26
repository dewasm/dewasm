// Resolve one import from the embedder's imports object. Returns null when the
// module or name is absent, so the caller falls through to its bundled-WASI /
// ENOSYS / link-error fallback (ADR-7).
static Object resolve_import(java.util.Map<String, java.util.Map<String, Object>> imports, String mod, String name) {
    if (imports == null) {
        return null;
    }
    java.util.Map<String, Object> m = imports.get(mod);
    if (m == null) {
        return null;
    }
    return m.get(name);
}

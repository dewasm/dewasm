// Resolve one import from the embedder's imports object. Returns nil when the
// module or name is absent, so the caller can fall through to its bundled-WASI
// / ENOSYS / link-error fallback (ADR-7).
func (rt) resolve_import(imports Imports, mod, name string) any {
    m := imports[mod]
    if m == nil {
        return nil
    }
    return m[name]
}

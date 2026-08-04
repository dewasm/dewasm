// requires: rt/link_error
// Resolve one import from the embedder's imports object. The source under a
// module name is either a name -> value map or an ImportProvider that resolves
// names itself (ADR-7). Returns nil when the module, or the name within it, is
// absent, so the caller can fall through to its bundled-WASI / ENOSYS /
// link-error fallback.
func (rt) resolve_import(imports Imports, mod, name string) any {
    switch source := imports[mod].(type) {
    case nil:
        return nil
    case map[string]any:
        return source[name]
    case Imports:
        return source[name]
    case ImportProvider:
        return source.WasmImport(name)
    default:
        Rt.link_error("import source for " + mod + " is neither a name map nor an ImportProvider")
        return nil
    }
}

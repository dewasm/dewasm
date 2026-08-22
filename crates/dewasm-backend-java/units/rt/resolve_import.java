// requires: rt/link_error
// Resolve one import from the embedder's imports object.
// The source under a module name is either a name -> value map or an ImportProvider that resolves names itself; the parameter is wildcarded so both a
// `Map<String, Map<String, Object>>` and a mixed `Map<String, Object>` are accepted.
// Returns null when the module, or the name within it, is absent, so the caller falls through to its bundled-WASI / ENOSYS / link-error fallback.
static Object resolve_import(java.util.Map<String, ?> imports, String mod, String name) {
    if (imports == null) {
        return null;
    }
    Object source = imports.get(mod);
    if (source == null) {
        return null;
    }
    if (source instanceof ImportProvider) {
        return ((ImportProvider) source).wasmImport(name);
    }
    if (source instanceof java.util.Map) {
        return ((java.util.Map<?, ?>) source).get(name);
    }
    link_error("import source for " + mod + " is neither a name map nor an Rt.ImportProvider");
    return null;
}

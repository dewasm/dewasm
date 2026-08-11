// Root runtime scope. `rt` is a zero-size receiver so the "static" helpers
// read as `Rt.name(...)` in generated code, mirroring Ruby/Python's `Rt.`
// prefix. Helper method names are the snake_case wasm op ids (legal Go,
// unexported) so a runtime unit id maps 1:1 to its reference, keeping the
// units lint a direct name match.
type rt struct{}

var Rt rt

// Imports is the embedder's import object: module -> source. A source is
// either a `map[string]any` (name -> value) or an ImportProvider resolving
// names itself. Function imports hold a Go func value of the wasm signature;
// resolution type-asserts to that exact type.
type Imports map[string]any

// ImportProvider is a module source that resolves names on demand, so one
// object can stand in for a whole module, the Go shape of the shared
// import-provider protocol (Ruby's `import`, Python's `wasm_import`).
// Returning nil for a name leaves that import unresolved, exactly as an absent
// map entry does, so the module still falls back to its bundled WASI / link
// error.
type ImportProvider interface {
    WasmImport(name string) any
}

// ImportAttacher is the optional second half of the provider protocol: a
// generated constructor calls Attach on every provider once the instance is
// fully built, so a provider can reach the instance (its memory, above all)
// without the embedder wiring a back-reference by hand.
type ImportAttacher interface {
    Attach(instance any)
}

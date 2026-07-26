// Root runtime scope. `rt` is a zero-size receiver so the "static" helpers
// read as `Rt.name(...)` in generated code, mirroring Ruby/Python's `Rt.`
// (ADR-29). Helper method names are the snake_case wasm op ids (legal Go,
// unexported) so a runtime unit id maps 1:1 to its reference, keeping the
// units lint a direct name match.
type rt struct{}

var Rt rt

// Imports is the embedder's import object: module -> name -> value. Function
// imports hold a Go func value of the wasm signature; resolution type-asserts
// to that exact type (ADR-7 / ADR-29).
type Imports map[string]map[string]any

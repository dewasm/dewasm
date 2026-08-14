// Identity object for a wasm exception tag: catch clauses compare tag pointers, so an imported tag matches its origin by sharing this object, never by structure.
// The blank field is load-bearing. Go may place two allocations of a zero-size type at the same address, which would make two distinct `(tag)` definitions compare equal.
type rtTag struct{ _ byte }

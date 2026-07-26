// A boxed global: a pointer-shared mutable cell, so a global that crosses an
// instantiation boundary (imported, or exported and later imported by another
// instance) is a shared cell rather than a copied value — the same reason
// Memory/Table are objects (ADR-16). Generic over the value type keeps every
// access statically typed (ADR-29): `p.g0.value` needs no assertion, and an
// imported global resolves by asserting `v.(*global[uint32])`, which also
// checks the value type (mutability and other finer limits stay unchecked —
// the import-limits gap).
type global[T any] struct {
	value T
}

func newGlobal[T any](v T) *global[T] {
	return &global[T]{value: v}
}

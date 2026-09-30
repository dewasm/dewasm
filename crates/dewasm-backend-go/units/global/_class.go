// A boxed global: a pointer-shared mutable cell.
// A global can cross an instantiation boundary.
// It is imported, or exported and later imported by another instance.
// Such a global is then a shared cell rather than a copied value.
// That is the same reason Memory/Table are objects.
// Generic over the value type keeps every access statically typed: `p.g0.value` needs no assertion.
// An imported global resolves by asserting `v.(*global[uint32])`, which also checks the value type.
// Mutability and other finer limits stay unchecked (the import-limits gap).
type global[T any] struct {
	value T
}

func newGlobal[T any](v T) *global[T] {
	return &global[T]{value: v}
}

// A boxed global is a shared mutable cell.
// So a global that crosses an instantiation boundary is one shared cell rather than a copied value.
// Such a global is imported, or exported and later imported by another instance.
// Memory/Table are objects for the same reason (mirroring Go's *global[T]).
// The value is stored boxed (`Object`).
// Java generics cannot hold a primitive, and the dynamic import boundary already boxes.
// An imported global resolves by an `instanceof Global` kind check.
// The boxed value's wasm type is not checked.
// That is the wider import-limits gap this backend already accepts.
Object value;

Global(Object value) {
    this.value = value;
}

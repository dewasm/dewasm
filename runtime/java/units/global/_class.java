// A boxed global: a shared mutable cell, so a global that crosses an
// instantiation boundary (imported, or exported and later imported by another
// instance) is one shared cell rather than a copied value — the same reason
// Memory/Table are objects (mirroring Go's *global[T]). The value is stored
// boxed (`Object`): Java generics cannot hold a primitive, and the dynamic
// import boundary already boxes. An imported global resolves by an
// `instanceof Global` kind check; the boxed value's wasm type is not checked
// (the wider import-limits gap this backend already accepts).
Object value;

Global(Object value) {
    this.value = value;
}

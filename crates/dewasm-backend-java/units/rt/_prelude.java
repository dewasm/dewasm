// Root runtime scope.
// `Rt` holds the static wasm helpers and the function-value interface (Fn).
// It also holds the `funcref` box for `call_indirect` and the trap/exit/link exception kinds.
// Generated code refers to these as `Rt.<name>` / `Rt.Fn` / `Rt.Funcref`.
// A wasm function value uses a boxed calling convention only at the dynamic boundary.
// That boundary is imports, `call_indirect`, and exports.
// There its arguments and result are `Object[]` and `Object`.
// Direct calls to defined functions stay primitive.
// Helper method names are the wasm instruction names in `snake_case` (legal Java).
// So a unit id maps 1:1 to its reference, and the units lint stays a direct name match.
// This matches Go, which also breaks its own naming style there.
interface Fn {
    Object invoke(Object[] args);
}

// A module source that resolves import names on demand.
// So one object can stand in for a whole module in the imports map.
// This is the Java shape of the shared import-provider protocol.
// Ruby's is `import`, and Python's is `wasm_import`.
// Returning null for a name leaves that import unresolved, as a missing map entry does.
// So the module still falls back to its bundled WASI / link error.
// A generated constructor calls `attach` on every provider once the instance is built.
// So a provider can reach the instance (its memory, above all).
// The embedder then need not set up a back-reference by hand.
interface ImportProvider {
    Object wasmImport(String name);

    default void attach(Object instance) {
    }
}

static final class Funcref {
    final String ty;
    final Fn fn;
    // The tail entry of a tail-calling function and the instance it belongs to.
    // Both are null for everything else, which completes in a single frame anyway.
    // The entry reads its owner's parked slots, so `table/tail_slot` only lets that owner park it.
    final Object body;
    final Object owner;

    Funcref(String ty, Fn fn) {
        this(ty, fn, null, null);
    }

    Funcref(String ty, Fn fn, Object body, Object owner) {
        this.ty = ty;
        this.fn = fn;
        this.body = body;
        this.owner = owner;
    }
}

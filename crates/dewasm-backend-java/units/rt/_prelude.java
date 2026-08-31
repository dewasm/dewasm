// Root runtime scope.
// `Rt` holds the static wasm helpers, the function-value interface (Fn), the funcref box for call_indirect, and the trap/exit/link exception kinds.
// Generated code refers to these as `Rt.<name>` /
// `Rt.Fn` / `Rt.Funcref`.
// A wasm function value uses a boxed calling convention only at the dynamic boundary (imports, call_indirect, exports):
// its args/result are `Object[]`/`Object`; direct calls to defined functions stay primitive.
// Helper method names are the snake_case wasm op ids (legal
// Java) so a unit id maps 1:1 to its reference, keeping the units lint a direct name match (mirroring Go's own deviation from idiom).
interface Fn {
    Object invoke(Object[] args);
}

// A module source that resolves import names on demand, so one object can stand in for a whole module in the imports map, the Java shape of the shared import-provider protocol (Ruby's `import`, Python's `wasm_import`).
// Returning null for a name leaves that import unresolved, exactly as an absent map entry does, so the module still falls back to its bundled WASI / link error.
// A generated constructor calls `attach` on every provider once the instance is fully built, so a provider can reach the instance (its memory, above all) without the embedder wiring a back-reference by hand.
interface ImportProvider {
    Object wasmImport(String name);

    default void attach(Object instance) {
    }
}

static final class Funcref {
    final String ty;
    final Fn fn;
    // The split body of a tail-calling function, which `table/tail_ref` hands to the trampoline so a chain through the table stays flat; null for everything else, which completes in a single frame anyway.
    final Fn body;

    Funcref(String ty, Fn fn) {
        this(ty, fn, null);
    }

    Funcref(String ty, Fn fn, Fn body) {
        this.ty = ty;
        this.fn = fn;
        this.body = body;
    }
}

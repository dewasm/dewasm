// Root runtime scope. `Rt` holds the static wasm helpers, the function-value
// interface (Fn), the funcref box for call_indirect, and the trap/exit/link
// exception kinds (ADR-30). Generated code refers to these as `Rt.<name>` /
// `Rt.Fn` / `Rt.Funcref`. A wasm function value uses a boxed calling
// convention only at the dynamic boundary (imports, call_indirect, exports):
// its args/result are `Object[]`/`Object`; direct calls to defined functions
// stay primitive. Helper method names are the snake_case wasm op ids (legal
// Java) so a unit id maps 1:1 to its reference, keeping the units lint a
// direct name match (ADR-30, mirroring Go's ADR-29 deviation from idiom).
interface Fn {
    Object invoke(Object[] args);
}

static final class Funcref {
    final String ty;
    final Fn fn;

    Funcref(String ty, Fn fn) {
        this.ty = ty;
        this.fn = fn;
    }
}

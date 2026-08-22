// requires: rt/tag
// A wasm exception in flight; the object itself is the exnref value, and its payload is boxed like every other value crossing a dynamic boundary.
// Deliberately unrelated to Trap and Exit: a try_table catches this class alone, so traps and the exit path structurally cannot be caught by catch_all.
// `wasm_exception` is void and throws (see rt/trap).
static final class WasmException extends RuntimeException {
    final Tag tag;
    final Object[] values;

    WasmException(Tag tag, Object[] values) {
        super("wasm exception");
        this.tag = tag;
        this.values = values;
    }
}

static void wasm_exception(Tag tag, Object[] values) {
    throw new WasmException(tag, values);
}

// requires: rt/trap, rt/wasm_exception
// Rethrow a caught exception; a null exception reference traps.
// `throw_ref` is void and throws (see rt/trap).
static void throw_ref(WasmException exn) {
    if (exn == null) {
        trap("null exception reference");
    }
    throw exn;
}

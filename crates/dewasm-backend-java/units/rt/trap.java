// A wasm trap: a runtime fault (out-of-bounds, integer overflow, ...).
// `trap` is `void` and throws, so a generated `Rt.trap(...)` statement needs no Java `throw`.
// A `throw` at the call site would make the statement after it an "unreachable statement" error.
// Recovered at the public boundary.
static final class Trap extends RuntimeException {
    Trap(String msg) {
        super(msg);
    }
}

static void trap(String msg) {
    throw new Trap(msg);
}

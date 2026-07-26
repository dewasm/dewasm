// A wasm trap: a runtime fault (out-of-bounds, integer overflow, ...). `trap`
// is void and throws, so a generated `Rt.trap(...)` statement needs no Java
// `throw` at the call site — which avoids an "unreachable statement" error
// after it (ADR-30). Recovered at the public boundary.
static final class Trap extends RuntimeException {
    Trap(String msg) {
        super(msg);
    }
}

static void trap(String msg) {
    throw new Trap(msg);
}

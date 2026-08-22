// A WASI proc_exit request, distinct from a trap: carries the exit code up to the public boundary.
// `exit` is void and throws (see rt/trap).
static final class Exit extends RuntimeException {
    final int code;

    Exit(int code) {
        super("proc_exit(" + code + ")");
        this.code = code;
    }
}

static void exit(int code) {
    throw new Exit(code);
}

// A failed import resolution at instantiation time (missing import, or one of the wrong kind), kept distinct from a trap.
// `link_error` is void and throws.
static final class LinkError extends RuntimeException {
    LinkError(String msg) {
        super(msg);
    }
}

static void link_error(String msg) {
    throw new LinkError(msg);
}

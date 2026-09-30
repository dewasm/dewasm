// A failed import resolution at instantiation time, kept distinct from a trap.
// The import is missing, or one of the wrong kind.
// `link_error` is void and throws.
static final class LinkError extends RuntimeException {
    LinkError(String msg) {
        super(msg);
    }
}

static void link_error(String msg) {
    throw new LinkError(msg);
}

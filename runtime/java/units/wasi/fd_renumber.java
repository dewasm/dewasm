// Move the fd `from` onto the number `to`, closing whatever `to` held first
// (ADR-40). Both must currently be open (renumbering onto an invalid target is
// EBADF); the table entry and its rights meta move together, and `from` is
// then closed. Renumbering onto stdio or a preopen is allowed — the target's
// old entry is simply replaced (only a guest-opened file carries a channel to
// close).
int wasi_fd_renumber(int from, int to) {
    if (!fds.containsKey(from) || !fds.containsKey(to)) {
        return WASI_BADF;
    }
    if (from == to) {
        return WASI_OK;
    }
    Object toEntry = fds.get(to);
    if (toEntry instanceof Handle) {
        try {
            ((Handle) toEntry).ch.close();
        } catch (java.io.IOException ex) {
            return WASI_IO;
        }
    }
    fds.put(to, fds.remove(from));
    FdMeta m = meta.remove(from);
    if (m != null) {
        meta.put(to, m);
    } else {
        meta.remove(to);
    }
    return WASI_OK;
}

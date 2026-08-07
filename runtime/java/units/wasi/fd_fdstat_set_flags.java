// Update an fd's open fdflags. Only APPEND is behaviorally honored:
// fd_write seeks to end before each write when Handle.append is set, so
// toggling APPEND here flips that. The remaining flags (DSYNC/RSYNC/SYNC,
// NONBLOCK) are stored so fd_fdstat_get reflects them but carry no distinct
// behavior in this runtime.
int wasi_fd_fdstat_set_flags(int fd, int flags) {
    Object e = fds.get(fd);
    if (!fds.containsKey(fd)) {
        return WASI_BADF;
    }
    FdMeta m = meta.get(fd);
    if (m != null) {
        m.fdflags = flags;
    }
    if (e instanceof Handle) {
        ((Handle) e).append = (flags & 0x1) != 0; // fdflags::APPEND
    }
    return WASI_OK;
}

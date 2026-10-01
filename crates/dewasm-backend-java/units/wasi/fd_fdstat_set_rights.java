// Narrow (never widen) a descriptor's stored capability rights.
// WASI rights are monotonically droppable.
// A request for any bit the descriptor does not currently hold is NOTCAPABLE.
// So a guest can give up authority but not regain it.
// The system calls that check rights then honor the narrowed set.
// Those are `fd_read`/`write`/`seek`/`readdir`, `fd_filestat_set_size`, and `path_open`.
int wasi_fd_fdstat_set_rights(int fd, long base, long inheriting) {
    if (!fds.containsKey(fd)) {
        return WASI_BADF;
    }
    FdMeta m = meta.get(fd);
    if (m == null) {
        // The inherited stdio streams carry no tracked rights; a set is a no-op.
        return WASI_OK;
    }
    if ((base & ~m.base) != 0 || (inheriting & ~m.inheriting) != 0) {
        return WASI_NOTCAPABLE;
    }
    m.base = base;
    m.inheriting = inheriting;
    return WASI_OK;
}

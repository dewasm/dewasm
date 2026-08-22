// Narrow (never widen) an fd's stored capability rights.
// WASI rights are monotonically droppable: a request for any bit the fd does not currently hold is NOTCAPABLE, so a guest can shed authority but not regain it.
// The enforcing syscalls (fd_read/write/seek/readdir, fd_filestat_set_size, path_open) then honor the narrowed set.
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

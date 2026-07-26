// requires: memory/fill, memory/i32_store8, memory/i64_store
int wasi_fd_fdstat_get(int fd, int outPtr) {
    Object e = fds.get(fd);
    if (e == null && !fds.containsKey(fd)) {
        return WASI_BADF;
    }
    int filetype = 4; // regular file
    if (e instanceof Dir) {
        filetype = 3; // directory
    } else if (isStdio(e)) {
        // A tty reports as a character device (2); a pipe/redirect reports as a
        // regular file (4), so guests' isatty() stays false under piped I/O —
        // matching the wasmtime golden captured with piped stdin (ADR-30).
        filetype = (System.console() != null) ? 2 : 4;
    }
    // fdstat: fs_filetype (u8) + pad + fs_flags (u16) + pad + fs_rights_base
    // (u64) + fs_rights_inheriting (u64) = 24 bytes.
    memory.fill(Integer.toUnsignedLong(outPtr), 0, 24);
    memory.i32_store8(Integer.toUnsignedLong(outPtr), filetype);
    memory.i64_store(Integer.toUnsignedLong(outPtr) + 8, -1L); // rights base: all
    memory.i64_store(Integer.toUnsignedLong(outPtr) + 16, -1L); // rights inheriting: all
    return WASI_OK;
}

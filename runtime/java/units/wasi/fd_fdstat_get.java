// requires: memory/fill, memory/i32_store8, memory/i32_store16, memory/i64_store
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
        // regular file (4), so guests' isatty() stays false under piped I/O,
        // matching the wasmtime snapshot captured with piped stdin.
        filetype = (System.console() != null) ? 2 : 4;
    }
    // The stored per-fd rights and open fdflags; an fd with no meta
    // (the inherited stdio streams) reports full rights and no flags.
    FdMeta m = meta.get(fd);
    long base = (m != null) ? m.base : -1L;
    long inheriting = (m != null) ? m.inheriting : -1L;
    int fdflags = (m != null) ? m.fdflags : 0;
    // fdstat: fs_filetype (u8) + pad + fs_flags (u16) + pad + fs_rights_base
    // (u64) + fs_rights_inheriting (u64) = 24 bytes.
    memory.fill(Integer.toUnsignedLong(outPtr), 0, 24);
    memory.i32_store8(Integer.toUnsignedLong(outPtr), filetype);
    memory.i32_store16(Integer.toUnsignedLong(outPtr) + 2, fdflags);
    memory.i64_store(Integer.toUnsignedLong(outPtr) + 8, base);
    memory.i64_store(Integer.toUnsignedLong(outPtr) + 16, inheriting);
    return WASI_OK;
}

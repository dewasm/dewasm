// Ensure the file is at least `offset+len` bytes.
// FileChannel has no `fallocate`.
// So a grow is done by a positioned one-byte write at newSize-1 (the gap is a sparse hole).
// That matches the grow-only contract of `posix_fallocate`, which never makes a file smaller.
// A request that does not exceed the current size is a no-op.
int wasi_fd_allocate(int fd, long offset, long len) {
    Object e = fds.get(fd);
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    java.nio.channels.FileChannel ch = ((Handle) e).ch;
    long newSize = offset + len;
    try {
        if (newSize > ch.size()) {
            ch.write(java.nio.ByteBuffer.wrap(new byte[] {0}), newSize - 1);
        }
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}

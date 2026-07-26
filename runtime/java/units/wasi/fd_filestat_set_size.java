// Truncate or extend a file to `size`. FileChannel.truncate only shrinks, so
// growing is done by writing one zero byte at size-1 (the gap is a sparse
// hole), matching ftruncate's grow semantics (ADR-14).
int wasi_fd_filestat_set_size(int fd, long size) {
    Object e = fds.get(fd);
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    java.nio.channels.FileChannel ch = ((Handle) e).ch;
    try {
        long cur = ch.size();
        if (size < cur) {
            ch.truncate(size);
        } else if (size > cur) {
            ch.write(java.nio.ByteBuffer.wrap(new byte[] {0}), size - 1);
        }
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}

// requires: memory/i32_load, memory/i32_store, memory/init
// Positional scatter read: reads at an absolute file offset without moving the channel's own position.
// Stdio is not seekable (SPIPE).
int wasi_fd_pread(int fd, int iovsPtr, int iovsLen, long offset, int nreadPtr) {
    Object e = fds.get(fd);
    if (isStdio(e)) {
        return WASI_SPIPE;
    }
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    if (lacksRight(fd, R_FD_READ)) {
        return WASI_NOTCAPABLE;
    }
    java.nio.channels.FileChannel ch = ((Handle) e).ch;
    int nread = 0;
    try {
        for (int i = 0; i < iovsLen; i++) {
            long base = Integer.toUnsignedLong(iovsPtr) + (long) i * 8;
            int ptr = memory.i32_load(base);
            int len = memory.i32_load(base + 4);
            if (len == 0) {
                continue;
            }
            byte[] buf = new byte[len];
            int n = ch.read(java.nio.ByteBuffer.wrap(buf), offset + Integer.toUnsignedLong(nread));
            if (n > 0) {
                memory.init(Integer.toUnsignedLong(ptr), buf, 0, n);
                nread += n;
            }
            // read returns -1 (a short read) at end of file; either ends the scatter.
            if (n < len) {
                break;
            }
        }
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    memory.i32_store(Integer.toUnsignedLong(nreadPtr), nread);
    return WASI_OK;
}

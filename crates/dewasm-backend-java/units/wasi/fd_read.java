// requires: memory/i32_load, memory/i32_store, memory/init
// Scatter read from stdin (an InputStream) or a guest-opened file (a
// FileChannel, advancing its position).
// A short read (or EOF) ends the scatter.
int wasi_fd_read(int fd, int iovsPtr, int iovsLen, int nreadPtr) {
    Object e = fds.get(fd);
    java.nio.channels.FileChannel ch = (e instanceof Handle) ? ((Handle) e).ch : null;
    java.io.InputStream in = (e instanceof java.io.InputStream) ? (java.io.InputStream) e : null;
    if (ch == null && in == null) {
        return WASI_BADF;
    }
    if (lacksRight(fd, R_FD_READ)) {
        return WASI_NOTCAPABLE;
    }
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
            int n = (in != null) ? in.read(buf) : ch.read(java.nio.ByteBuffer.wrap(buf));
            if (n > 0) {
                memory.init(Integer.toUnsignedLong(ptr), buf, 0, n);
                nread += n;
            }
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

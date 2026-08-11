// requires: memory/i32_load, memory/i32_store, memory/read_string
// Gather write to stdout/stderr (an OutputStream) or a guest-opened file (a
// FileChannel).
// For an O_APPEND file the position is moved to end before the write, reproducing append semantics without the FileChannel APPEND option
// (which forbids combining with READ/TRUNCATE).
int wasi_fd_write(int fd, int iovsPtr, int iovsLen, int nwrittenPtr) {
    Object e = fds.get(fd);
    java.io.OutputStream s = (e instanceof java.io.OutputStream) ? (java.io.OutputStream) e : null;
    Handle h = (e instanceof Handle) ? (Handle) e : null;
    if (s == null && h == null) {
        return WASI_BADF;
    }
    if (lacksRight(fd, R_FD_WRITE)) {
        return WASI_NOTCAPABLE;
    }
    int written = 0;
    try {
        if (h != null && h.append) {
            h.ch.position(h.ch.size());
        }
        for (int i = 0; i < iovsLen; i++) {
            long base = Integer.toUnsignedLong(iovsPtr) + (long) i * 8;
            int ptr = memory.i32_load(base);
            int len = memory.i32_load(base + 4);
            byte[] chunk = memory.read_string(Integer.toUnsignedLong(ptr), Integer.toUnsignedLong(len));
            if (s != null) {
                s.write(chunk);
            } else {
                java.nio.ByteBuffer bb = java.nio.ByteBuffer.wrap(chunk);
                while (bb.hasRemaining()) {
                    h.ch.write(bb);
                }
            }
            written += len;
        }
        if (s != null) {
            s.flush();
        }
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    memory.i32_store(Integer.toUnsignedLong(nwrittenPtr), written);
    return WASI_OK;
}

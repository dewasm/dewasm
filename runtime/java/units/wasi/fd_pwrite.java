// requires: memory/i32_load, memory/i32_store, memory/read_string
// Positional gather write: writes at an absolute file offset without moving the
// channel's own position. Stdio is not seekable (SPIPE).
int wasi_fd_pwrite(int fd, int iovsPtr, int iovsLen, long offset, int nwrittenPtr) {
    Object e = fds.get(fd);
    if (isStdio(e)) {
        return WASI_SPIPE;
    }
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    java.nio.channels.FileChannel ch = ((Handle) e).ch;
    int written = 0;
    try {
        for (int i = 0; i < iovsLen; i++) {
            long base = Integer.toUnsignedLong(iovsPtr) + (long) i * 8;
            int ptr = memory.i32_load(base);
            int len = memory.i32_load(base + 4);
            byte[] chunk = memory.read_string(Integer.toUnsignedLong(ptr), Integer.toUnsignedLong(len));
            java.nio.ByteBuffer bb = java.nio.ByteBuffer.wrap(chunk);
            long pos = offset + Integer.toUnsignedLong(written);
            while (bb.hasRemaining()) {
                pos += ch.write(bb, pos);
            }
            written += len;
        }
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    memory.i32_store(Integer.toUnsignedLong(nwrittenPtr), written);
    return WASI_OK;
}

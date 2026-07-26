// requires: memory/i32_load, memory/i32_store, memory/read_string
int wasi_fd_write(int fd, int iovsPtr, int iovsLen, int nwrittenPtr) {
    java.io.OutputStream s;
    if (fd == 1) {
        s = stdout;
    } else if (fd == 2) {
        s = stderr;
    } else {
        return WASI_BADF;
    }
    int written = 0;
    try {
        for (int i = 0; i < iovsLen; i++) {
            long base = Integer.toUnsignedLong(iovsPtr) + (long) i * 8;
            int ptr = memory.i32_load(base);
            int len = memory.i32_load(base + 4);
            s.write(memory.read_string(Integer.toUnsignedLong(ptr), Integer.toUnsignedLong(len)));
            written += len;
        }
        s.flush();
    } catch (java.io.IOException e) {
        return WASI_IO;
    }
    memory.i32_store(Integer.toUnsignedLong(nwrittenPtr), written);
    return WASI_OK;
}

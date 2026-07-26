// requires: memory/i32_load, memory/i32_store, memory/init
int wasi_fd_read(int fd, int iovsPtr, int iovsLen, int nreadPtr) {
    if (fd != 0) {
        return WASI_BADF;
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
            int n = stdin.read(buf);
            if (n > 0) {
                memory.init(Integer.toUnsignedLong(ptr), buf, 0, n);
                nread += n;
            }
            if (n < len) {
                break;
            }
        }
    } catch (java.io.IOException e) {
        return WASI_IO;
    }
    memory.i32_store(Integer.toUnsignedLong(nreadPtr), nread);
    return WASI_OK;
}

// requires: memory/init
int wasi_fd_prestat_get(int fd, int outPtr) {
    Object e = fds.get(fd);
    if (!(e instanceof Dir) || ((Dir) e).preopenName == null) {
        return WASI_BADF;
    }
    byte[] name = ((Dir) e).preopenName;
    // prestat: tag (u8, 0 = dir) + 3 pad + pr_name_len (u32).
    byte[] buf = new byte[8];
    java.nio.ByteBuffer.wrap(buf).order(java.nio.ByteOrder.LITTLE_ENDIAN).putInt(4, name.length);
    memory.init(Integer.toUnsignedLong(outPtr), buf, 0, 8);
    return WASI_OK;
}

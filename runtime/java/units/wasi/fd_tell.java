// requires: memory/i64_store
int wasi_fd_tell(int fd, int outPtr) {
    Object e = fds.get(fd);
    if (isStdio(e)) {
        return WASI_SPIPE;
    }
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    try {
        memory.i64_store(Integer.toUnsignedLong(outPtr), ((Handle) e).ch.position());
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}

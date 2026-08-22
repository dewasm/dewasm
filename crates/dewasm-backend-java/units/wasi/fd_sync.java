int wasi_fd_sync(int fd) {
    Object e = fds.get(fd);
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    try {
        ((Handle) e).ch.force(true);
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}

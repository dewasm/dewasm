// force(false) flushes the file content without forcing metadata, the
// datasync guarantee.
int wasi_fd_datasync(int fd) {
    Object e = fds.get(fd);
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    try {
        ((Handle) e).ch.force(false);
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}

// Stdio and directory fds carry no OS handle to close.
// Fds are never reused after close.
int wasi_fd_close(int fd) {
    if (!fds.containsKey(fd)) {
        return WASI_BADF;
    }
    Object e = fds.remove(fd);
    meta.remove(fd);
    if (e instanceof Handle) {
        try {
            ((Handle) e).ch.close();
        } catch (java.io.IOException ex) {
            return WASI_IO;
        }
    }
    return WASI_OK;
}

// requires: memory/init, wasi/pack_filestat
int wasi_fd_filestat_get(int fd, int bufPtr) {
    Object e = fds.get(fd);
    java.nio.file.Path p;
    if (e instanceof Dir) {
        p = ((Dir) e).hostPath;
    } else if (e instanceof Handle) {
        p = ((Handle) e).path;
    } else if (isStdio(e)) {
        // Stdio has no host path to stat; report a zeroed filestat tagged as a
        // character device (filetype 2), the honest best-effort for an
        // inherited stream (ADR-30).
        byte[] buf = new byte[64];
        buf[16] = 2;
        memory.init(Integer.toUnsignedLong(bufPtr), buf, 0, 64);
        return WASI_OK;
    } else {
        return WASI_BADF;
    }
    try {
        memory.init(Integer.toUnsignedLong(bufPtr), pack_filestat(p, true), 0, 64);
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}

// requires: memory/i64_store
// WASI whence: 0=set, 1=cur, 2=end. FileChannel has no whence, so compute the
// absolute position. Stdio is not seekable (SPIPE).
int wasi_fd_seek(int fd, long offset, int whence, int outPtr) {
    Object e = fds.get(fd);
    if (isStdio(e)) {
        return WASI_SPIPE;
    }
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    java.nio.channels.FileChannel ch = ((Handle) e).ch;
    try {
        long pos;
        switch (whence) {
            case 0:
                pos = offset;
                break;
            case 1:
                pos = ch.position() + offset;
                break;
            case 2:
                pos = ch.size() + offset;
                break;
            default:
                return WASI_INVAL;
        }
        ch.position(pos);
        memory.i64_store(Integer.toUnsignedLong(outPtr), pos);
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}

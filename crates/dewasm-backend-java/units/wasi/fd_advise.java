// Access-pattern advice is advisory only: this runtime keeps no page cache to tune, so a valid request is accepted as a no-op.
// The fd must be a regular file and the advice one of the six WASI codes (NORMAL..NOREUSE).
int wasi_fd_advise(int fd, long offset, long len, int advice) {
    Object e = fds.get(fd);
    if (!(e instanceof Handle)) {
        return WASI_BADF;
    }
    if (advice < 0 || advice > 5) {
        return WASI_INVAL;
    }
    return WASI_OK;
}

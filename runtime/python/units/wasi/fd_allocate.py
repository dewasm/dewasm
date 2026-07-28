# requires: wasi/errno_fs
def wasi_fd_allocate(self, fd, offset, length):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    # fallocate never shrinks: grow the file to offset+len when that exceeds the
    # current size, otherwise leave it untouched (ADR-40).
    try:
        if offset + length > os.fstat(io.fileno()).st_size:
            os.ftruncate(io.fileno(), offset + length)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS

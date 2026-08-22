def wasi_fd_datasync(self, fd):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    try:
        if hasattr(os, "fdatasync"):
            os.fdatasync(io.fileno())
        else:
            os.fsync(io.fileno())  # not available on all platforms (macOS)
    except OSError:
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

def wasi_fd_sync(self, fd):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    try:
        os.fsync(io.fileno())
    except OSError:
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

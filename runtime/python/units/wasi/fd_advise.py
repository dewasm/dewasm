def wasi_fd_advise(self, fd, offset, length, advice):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    # posix_fadvise is purely advisory; validating the fd and succeeding is a
    # faithful no-op.
    return self.ERRNO_SUCCESS

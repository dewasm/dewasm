def wasi_fd_advise(self, fd, offset, length, advice):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    # `posix_fadvise` is only a hint; validating the descriptor and succeeding is a correct no-op.
    return self.ERRNO_SUCCESS

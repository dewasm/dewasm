def wasi_fd_close(self, fd):
    io = self.fds.pop(fd, None)
    if io is None:
        return self.ERRNO_BADF
    if isinstance(io, self.WasiDir):
        return self.ERRNO_SUCCESS  # no real OS handle to close
    if io not in self.std_ios:
        io.close()
    return self.ERRNO_SUCCESS

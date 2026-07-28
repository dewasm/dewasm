def wasi_fd_fdstat_set_flags(self, fd, flags):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    # Store the new fdflags; fd_write consults fdflags::APPEND before each write
    # so clearing it here (set_flags 0) actually turns append off (ADR-40).
    self.fd_meta[fd][2] = flags & 0xFFFF
    return self.ERRNO_SUCCESS

# Any open fd is accepted, stdio included: toywasm's WASI setup sets NONBLOCK on stdin and treats failure as fatal, so wasmtime's regular-files-only EBADF answer is not copied.
def wasi_fd_fdstat_set_flags(self, fd, flags):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    # Store the new fdflags; fd_write consults fdflags::APPEND before each write so clearing it here (set_flags 0) actually turns append off.
    self.fd_meta[fd][2] = flags & 0xFFFF
    return self.ERRNO_SUCCESS

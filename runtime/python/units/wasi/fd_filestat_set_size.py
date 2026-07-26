def wasi_fd_filestat_set_size(self, fd, size):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    try:
        os.ftruncate(io.fileno(), size)
    except OSError:
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

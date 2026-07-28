def wasi_fd_filestat_set_size(self, fd, size):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    if not (self.fd_meta[fd][0] & self.RIGHTS_FD_FILESTAT_SET_SIZE):
        return self.ERRNO_NOTCAPABLE
    try:
        os.ftruncate(io.fileno(), size)
    except OSError:
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

# requires: wasi/fst_times
def wasi_fd_filestat_set_times(self, fd, atim, mtim, fst_flags):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    try:
        st = os.fstat(io.fileno())
    except OSError:
        return self.ERRNO_IO
    times, err = self.fst_times(st, atim, mtim, fst_flags)
    if err is not None:
        return err
    try:
        os.utime(io.fileno(), ns=times)
    except OSError:
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

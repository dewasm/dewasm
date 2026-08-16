# requires: memory/ids
def wasi_fd_tell(self, fd, out_ptr):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    if io in self.std_ios:
        return self.ERRNO_SPIPE
    try:
        self.memory.ids(out_ptr, io.tell() & Rt.M64)
    except OSError:
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

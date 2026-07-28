# requires: memory/i64_store, rt/s64
def wasi_fd_seek(self, fd, offset, whence, out_ptr):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    if io in self.std_ios:
        return self.ERRNO_SPIPE
    if not (self.fd_meta[fd][0] & self.RIGHTS_FD_SEEK):
        return self.ERRNO_NOTCAPABLE
    if whence == 0:
        mode = os.SEEK_SET
    elif whence == 1:
        mode = os.SEEK_CUR
    elif whence == 2:
        mode = os.SEEK_END
    else:
        return self.ERRNO_INVAL
    try:
        io.seek(Rt.s64(offset), mode)
        self.memory.i64_store(out_ptr, io.tell() & Rt.M64)
    except OSError as e:
        # A negative resulting offset is EINVAL, which WASI reports as INVAL
        # (not the generic IO used for other seek failures).
        if e.errno == errno.EINVAL:
            return self.ERRNO_INVAL
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

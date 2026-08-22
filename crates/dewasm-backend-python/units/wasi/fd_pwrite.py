# requires: memory/iwl, memory/iws, memory/read_string
def wasi_fd_pwrite(self, fd, iovs_ptr, iovs_len, offset, nwritten_ptr):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    if io in self.std_ios:
        return self.ERRNO_SPIPE
    if not (self.fd_meta[fd][0] & self.RIGHTS_FD_WRITE):
        return self.ERRNO_NOTCAPABLE
    written = 0
    try:
        for i in range(iovs_len):
            ptr = self.memory.iwl(iovs_ptr + i * 8)
            length = self.memory.iwl(iovs_ptr + i * 8 + 4)
            chunk = self.memory.read_string(ptr, length)
            n = os.pwrite(io.fileno(), chunk, offset + written)
            written += n
            # A single pwrite(2) may write short; stop so the reported nwritten stays contiguous.
            if n < len(chunk):
                break
    except OSError:
        return self.ERRNO_IO
    self.memory.iws(nwritten_ptr, written)
    return self.ERRNO_SUCCESS

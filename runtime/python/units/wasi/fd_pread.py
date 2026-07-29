# requires: memory/i32_load, memory/i32_store, memory/init
def wasi_fd_pread(self, fd, iovs_ptr, iovs_len, offset, nread_ptr):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    if io in self.std_ios:
        return self.ERRNO_SPIPE
    if not (self.fd_meta[fd][0] & self.RIGHTS_FD_READ):
        return self.ERRNO_NOTCAPABLE
    nread = 0
    try:
        for i in range(iovs_len):
            ptr = self.memory.i32_load(iovs_ptr + i * 8)
            length = self.memory.i32_load(iovs_ptr + i * 8 + 4)
            if length == 0:
                continue
            # os.pread returns b"" at end-of-file (a short/empty read).
            chunk = os.pread(io.fileno(), length, offset + nread)
            if not chunk:
                break
            self.memory.init(ptr, chunk, 0, len(chunk))
            nread += len(chunk)
            if len(chunk) < length:
                break
    except OSError:
        return self.ERRNO_IO
    self.memory.i32_store(nread_ptr, nread)
    return self.ERRNO_SUCCESS

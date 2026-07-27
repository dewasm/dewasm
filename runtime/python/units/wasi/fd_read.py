# requires: memory/i32_load, memory/i32_store, memory/init
def wasi_fd_read(self, fd, iovs_ptr, iovs_len, nread_ptr):
    io = self.fds.get(fd)
    if io is None or isinstance(io, self.WasiDir):
        return self.ERRNO_BADF
    stdin = io is self.std_ios[0]
    nread = 0
    try:
        for i in range(iovs_len):
            ptr = self.memory.i32_load(iovs_ptr + i * 8)
            length = self.memory.i32_load(iovs_ptr + i * 8 + 4)
            if length == 0:
                continue
            # stdin may be an interactive tty (the QuickJS REPL under a pty): a
            # buffered read(length) blocks until length bytes arrive or EOF,
            # deadlocking a line-buffered terminal that never sends EOF. A raw
            # os.read returns as soon as any bytes are available — the WASI
            # short-read semantics wasmtime uses — while poll_oneoff's select
            # already gated the wait. Files keep the buffered read.
            if stdin:
                chunk = os.read(io.fileno(), length)
            else:
                chunk = io.read(length)
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

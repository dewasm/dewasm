# requires: memory/fill, memory/i32_store8, memory/i64_store
def wasi_fd_fdstat_get(self, fd, out_ptr):
    io = self.fds.get(fd)
    if io is None:
        return self.ERRNO_BADF
    if isinstance(io, self.WasiDir):
        filetype = 3  # directory
    else:
        try:
            is_tty = io.isatty()
        except (AttributeError, OSError, ValueError):
            is_tty = False
        filetype = 2 if is_tty else 4  # char device / regular file
    # fdstat: fs_filetype (u8) + pad + fs_flags (u16) + pad + fs_rights_base
    # (u64) + fs_rights_inheriting (u64) = 24 bytes.
    self.memory.fill(out_ptr, 0, 24)
    self.memory.i32_store8(out_ptr, filetype)
    self.memory.i64_store(out_ptr + 8, Rt.M64)   # rights base: everything
    self.memory.i64_store(out_ptr + 16, Rt.M64)  # rights inheriting: everything
    return self.ERRNO_SUCCESS

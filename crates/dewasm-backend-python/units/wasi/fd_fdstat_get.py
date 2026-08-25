# requires: memory/fill, memory/iwsb, memory/iwsh, memory/ids
def wasi_fd_fdstat_get(self, fd, out_ptr):
    io = self.fds.get(fd)
    if io is None:
        return self.ERRNO_BADF
    meta = self.fd_meta[fd]
    base, inheriting, fdflags = meta[0], meta[1], meta[2]
    # isatty() is a host syscall, and an open descriptor's filetype cannot change while it is open, so it runs at most once per fd (see the fourth meta slot in wasi/_class): a guest polling isatty in a loop would otherwise pay one syscall per call.
    filetype = meta[3]
    if filetype is None:
        if isinstance(io, self.WasiDir):
            filetype = 3  # directory
        else:
            try:
                is_tty = io.isatty()
            except (AttributeError, OSError, ValueError):
                is_tty = False
            filetype = 2 if is_tty else 4  # char device / regular file
        meta[3] = filetype
    # fdstat: fs_filetype (u8) + pad + fs_flags (u16) + pad + fs_rights_base
    # (u64) + fs_rights_inheriting (u64) = 24 bytes.
    self.memory.fill(out_ptr, 0, 24)
    self.memory.iwsb(out_ptr, filetype)
    self.memory.iwsh(out_ptr + 2, fdflags)
    self.memory.ids(out_ptr + 8, base)
    self.memory.ids(out_ptr + 16, inheriting)
    return self.ERRNO_SUCCESS

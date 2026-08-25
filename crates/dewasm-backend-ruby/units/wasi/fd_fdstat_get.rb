# requires: memory/fill, memory/iwsb, memory/iwsh, memory/ids
def wasi_fd_fdstat_get(fd, out_ptr)
  io = @fds[fd]
  return ERRNO_BADF unless io
  meta = @fd_meta[fd]
  base, inheriting, fdflags = meta || [Rt::M64, Rt::M64, 0]
  # `tty?` is a host syscall, and an open descriptor's filetype cannot change while it is open, so it runs at most once per fd (see the fourth meta slot in wasi/_class): a guest polling isatty in a loop would otherwise pay one syscall per call.
  filetype = meta && meta[3]
  unless filetype
    filetype =
      if io.is_a?(WasiDir)
        3 # directory
      else
        io.respond_to?(:tty?) && io.tty? ? 2 : 4 # char device / regular file
      end
    meta[3] = filetype if meta
  end
  @memory.fill(out_ptr, 0, 24)
  @memory.iwsb(out_ptr, filetype)
  @memory.iwsh(out_ptr + 2, fdflags)
  @memory.ids(out_ptr + 8, base)
  @memory.ids(out_ptr + 16, inheriting)
  ERRNO_SUCCESS
end

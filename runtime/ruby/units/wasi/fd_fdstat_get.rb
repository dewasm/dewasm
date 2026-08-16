# requires: memory/fill, memory/iwsb, memory/iwsh, memory/ids
def wasi_fd_fdstat_get(fd, out_ptr)
  io = @fds[fd]
  return ERRNO_BADF unless io
  filetype =
    if io.is_a?(WasiDir)
      3 # directory
    else
      io.respond_to?(:tty?) && io.tty? ? 2 : 4 # char device / regular file
    end
  base, inheriting, fdflags = @fd_meta[fd] || [Rt::M64, Rt::M64, 0]
  @memory.fill(out_ptr, 0, 24)
  @memory.iwsb(out_ptr, filetype)
  @memory.iwsh(out_ptr + 2, fdflags)
  @memory.ids(out_ptr + 8, base)
  @memory.ids(out_ptr + 16, inheriting)
  ERRNO_SUCCESS
end

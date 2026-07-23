# requires: memory/fill, memory/i32_store8, memory/i64_store
def wasi_fd_fdstat_get(fd, out_ptr)
  io = @fds[fd]
  return ERRNO_BADF unless io
  filetype =
    if io.is_a?(WasiDir)
      3 # directory
    else
      io.respond_to?(:tty?) && io.tty? ? 2 : 4 # char device / regular file
    end
  @memory.fill(out_ptr, 0, 24)
  @memory.i32_store8(out_ptr, filetype)
  @memory.i64_store(out_ptr + 8, Rt::M64)  # rights base: everything
  @memory.i64_store(out_ptr + 16, Rt::M64) # rights inheriting: everything
  ERRNO_SUCCESS
end

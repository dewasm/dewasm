# requires: memory/i64_store, rt/m64
def wasi_fd_tell(fd, out_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  return ERRNO_SPIPE if @std_ios.include?(io)
  @memory.i64_store(out_ptr, Rt.m64(io.tell))
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

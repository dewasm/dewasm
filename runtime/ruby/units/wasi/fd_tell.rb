# requires: memory/i64_store
def wasi_fd_tell(fd, out_ptr)
  io = @fds[fd]
  return ERRNO_BADF unless io
  @memory.i64_store(out_ptr, io.tell & Rt::M64)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

# requires: memory/i32_load, memory/i32_store, memory/read_string
def wasi_fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  written = 0
  iovs_len.times do |i|
    ptr = @memory.i32_load(iovs_ptr + i * 8)
    len = @memory.i32_load(iovs_ptr + i * 8 + 4)
    written += io.write(@memory.read_string(ptr, len))
  end
  io.flush
  @memory.i32_store(nwritten_ptr, written)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

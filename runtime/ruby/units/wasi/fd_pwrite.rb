# requires: memory/i32_load, memory/i32_store, memory/read_string
def wasi_fd_pwrite(fd, iovs_ptr, iovs_len, offset, nwritten_ptr)
  io = @fds[fd]
  return ERRNO_BADF unless io.is_a?(IO)
  return ERRNO_SPIPE if [$stdin, $stdout, $stderr].include?(io)
  written = 0
  iovs_len.times do |i|
    ptr = @memory.i32_load(iovs_ptr + i * 8)
    len = @memory.i32_load(iovs_ptr + i * 8 + 4)
    written += io.pwrite(@memory.read_string(ptr, len), offset + written)
  end
  @memory.i32_store(nwritten_ptr, written)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

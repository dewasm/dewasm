# requires: memory/i32_load, memory/i32_store, memory/init
def wasi_fd_read(fd, iovs_ptr, iovs_len, nread_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  nread = 0
  iovs_len.times do |i|
    ptr = @memory.i32_load(iovs_ptr + i * 8)
    len = @memory.i32_load(iovs_ptr + i * 8 + 4)
    next if len == 0
    chunk = io.read(len)
    break if chunk.nil?
    @memory.init(ptr, chunk, 0, chunk.bytesize)
    nread += chunk.bytesize
    break if chunk.bytesize < len
  end
  @memory.i32_store(nread_ptr, nread)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

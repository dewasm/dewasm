# requires: memory/iwl, memory/iws, memory/read_string, wasi/rights
def wasi_fd_pwrite(fd, iovs_ptr, iovs_len, offset, nwritten_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  return ERRNO_SPIPE if @std_ios.include?(io)
  return ERRNO_NOTCAPABLE unless fd_has_right?(fd, RIGHT_FD_WRITE)
  written = 0
  iovs_len.times do |i|
    ptr = @memory.iwl(iovs_ptr + i * 8)
    len = @memory.iwl(iovs_ptr + i * 8 + 4)
    chunk = @memory.read_string(ptr, len)
    n = io.pwrite(chunk, offset + written)
    written += n
    # IO#pwrite is a single pwrite(2), which may write short; stop so the reported nwritten stays contiguous.
    break if n < chunk.bytesize
  end
  @memory.iws(nwritten_ptr, written)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

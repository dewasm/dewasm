# requires: memory/iwl, memory/iws, memory/init, wasi/rights
def wasi_fd_pread(fd, iovs_ptr, iovs_len, offset, nread_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  return ERRNO_SPIPE if @std_ios.include?(io)
  return ERRNO_NOTCAPABLE unless fd_has_right?(fd, RIGHT_FD_READ)
  nread = 0
  iovs_len.times do |i|
    ptr = @memory.iwl(iovs_ptr + i * 8)
    len = @memory.iwl(iovs_ptr + i * 8 + 4)
    next if len == 0
    # IO#pread raises EOFError at end-of-file instead of returning a short/empty read like IO#read does.
    chunk = begin
      io.pread(len, offset + nread)
    rescue EOFError
      nil
    end
    break if chunk.nil? || chunk.empty?
    @memory.init(ptr, chunk, 0, chunk.bytesize)
    nread += chunk.bytesize
    break if chunk.bytesize < len
  end
  @memory.iws(nread_ptr, nread)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

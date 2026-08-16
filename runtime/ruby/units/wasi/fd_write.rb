# requires: memory/iwl, memory/iws, memory/read_string, wasi/rights
def wasi_fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  return ERRNO_NOTCAPABLE unless fd_has_right?(fd, RIGHT_FD_WRITE)
  # APPEND is implemented here rather than via O_APPEND (so fd_fdstat_set_flags can turn it off): seek to end before writing.
  io.seek(0, IO::SEEK_END) if (@fd_meta[fd][2] & 0x1) != 0
  written = 0
  iovs_len.times do |i|
    ptr = @memory.iwl(iovs_ptr + i * 8)
    len = @memory.iwl(iovs_ptr + i * 8 + 4)
    written += io.write(@memory.read_string(ptr, len))
  end
  io.flush
  @memory.iws(nwritten_ptr, written)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

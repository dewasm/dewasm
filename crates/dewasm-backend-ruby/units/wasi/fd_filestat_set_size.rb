# requires: wasi/rights
def wasi_fd_filestat_set_size(fd, size)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  return ERRNO_NOTCAPABLE unless fd_has_right?(fd, RIGHT_FD_FILESTAT_SET_SIZE)
  io.truncate(size)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

def wasi_fd_filestat_set_size(fd, size)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  io.truncate(size)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

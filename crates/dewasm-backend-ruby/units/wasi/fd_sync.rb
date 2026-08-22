def wasi_fd_sync(fd)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  io.fsync
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

def wasi_fd_sync(fd)
  io = @fds[fd]
  return ERRNO_BADF unless io.is_a?(IO)
  io.fsync
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

def wasi_fd_datasync(fd)
  io = @fds[fd]
  return ERRNO_BADF unless io.is_a?(IO)
  begin
    io.fdatasync
  rescue NotImplementedError
    io.fsync # not available on all platforms (e.g. macOS)
  end
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

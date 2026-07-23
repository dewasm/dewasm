def wasi_fd_datasync(fd)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  begin
    io.fdatasync
  rescue NotImplementedError, NoMethodError
    io.fsync # not available on all platforms (e.g. macOS) or IO-likes
  end
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

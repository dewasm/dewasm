def wasi_fd_advise(fd, _offset, _len, _advice)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  # Access-pattern advice is a hint; accepting it after validating the fd is a correct no-op implementation.
  ERRNO_SUCCESS
end

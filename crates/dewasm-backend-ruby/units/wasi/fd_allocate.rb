def wasi_fd_allocate(fd, offset, len)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  # fd_allocate only ever grows the file to at least offset+len; a request that ends within the current size is a no-op.
  needed = offset + len
  io.truncate(needed) if needed > io.size
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

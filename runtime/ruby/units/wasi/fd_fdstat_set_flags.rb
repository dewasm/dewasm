def wasi_fd_fdstat_set_flags(fd, flags)
  meta = @fd_meta[fd]
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir) || meta.nil?
  # Store the fdflags word; APPEND is honored by fd_write (which seeks to
  # end when the bit is set), so clearing it here disables append mode.
  meta[2] = flags & 0x1f
  ERRNO_SUCCESS
end

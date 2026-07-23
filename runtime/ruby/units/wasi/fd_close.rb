def wasi_fd_close(fd)
  io = @fds.delete(fd)
  return ERRNO_BADF unless io
  io.close unless [$stdin, $stdout, $stderr].include?(io)
  ERRNO_SUCCESS
end

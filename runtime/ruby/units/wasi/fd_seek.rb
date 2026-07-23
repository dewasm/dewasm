# requires: memory/i64_store, rt/s64
def wasi_fd_seek(fd, offset, whence, out_ptr)
  io = @fds[fd]
  return ERRNO_BADF unless io.is_a?(IO)
  return ERRNO_SPIPE if [$stdin, $stdout, $stderr].include?(io)
  mode = [IO::SEEK_SET, IO::SEEK_CUR, IO::SEEK_END][whence]
  return ERRNO_INVAL unless mode
  io.seek(Rt.s64(offset), mode)
  @memory.i64_store(out_ptr, io.tell & Rt::M64)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

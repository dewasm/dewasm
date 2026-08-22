# requires: memory/ids, rt/s64, rt/m64, wasi/rights
def wasi_fd_seek(fd, offset, whence, out_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  return ERRNO_SPIPE if @std_ios.include?(io)
  return ERRNO_NOTCAPABLE unless fd_has_right?(fd, RIGHT_FD_SEEK)
  mode = [IO::SEEK_SET, IO::SEEK_CUR, IO::SEEK_END][whence]
  return ERRNO_INVAL unless mode
  io.seek(Rt.s64(offset), mode)
  @memory.ids(out_ptr, Rt.m64(io.tell))
  ERRNO_SUCCESS
rescue Errno::EINVAL
  # Seeking before byte 0 raises EINVAL; surface it precisely rather than folding it into the generic ERRNO_IO.
  ERRNO_INVAL
rescue SystemCallError
  ERRNO_IO
end

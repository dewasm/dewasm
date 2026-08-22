# requires: memory/init
def wasi_fd_prestat_get(fd, out_ptr)
  entry = @fds[fd]
  return ERRNO_BADF unless entry.is_a?(WasiDir) && entry.preopen_name
  # prestat: tag (u8, 0 = dir) + 3 pad + pr_name_len (u32)
  @memory.init(out_ptr, [0, entry.preopen_name.bytesize].pack("Cx3L<"), 0, 8)
  ERRNO_SUCCESS
end

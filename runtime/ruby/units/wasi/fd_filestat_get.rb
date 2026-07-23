# requires: memory/init, wasi/pack_filestat
def wasi_fd_filestat_get(fd, buf_ptr)
  entry = @fds[fd]
  return ERRNO_BADF unless entry
  stat = entry.is_a?(WasiDir) ? File.stat(entry.host_path) : entry.stat
  @memory.init(buf_ptr, pack_filestat(stat), 0, 64)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

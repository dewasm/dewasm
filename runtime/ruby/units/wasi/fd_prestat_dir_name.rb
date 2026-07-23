# requires: memory/init, wasi/errno_fs
def wasi_fd_prestat_dir_name(fd, path_ptr, path_len)
  entry = @fds[fd]
  return ERRNO_BADF unless entry.is_a?(WasiDir) && entry.preopen_name
  name = entry.preopen_name
  return ERRNO_NAMETOOLONG if name.bytesize > path_len
  @memory.init(path_ptr, name, 0, name.bytesize)
  ERRNO_SUCCESS
end

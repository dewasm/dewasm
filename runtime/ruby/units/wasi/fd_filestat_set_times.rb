# requires: wasi/set_times, wasi/errno_fs
def wasi_fd_filestat_set_times(fd, atim, mtim, fstflags)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  err = validate_fstflags(fstflags)
  return err if err
  a, m = resolve_times(io.stat, atim, mtim, fstflags)
  File.utime(a, m, io.path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

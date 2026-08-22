# requires: memory/read_string, wasi/resolve_path, wasi/set_times, wasi/errno_fs
def wasi_path_filestat_set_times(dirfd, flags, path_ptr, path_len, atim, mtim, fstflags)
  rel = @memory.read_string(path_ptr, path_len)
  err = validate_fstflags(fstflags)
  return err if err
  symlink_follow = flags & 0x1 != 0 # lookupflags::SYMLINK_FOLLOW
  host_path, resolve_err = resolve_path(dirfd, rel, follow_last: symlink_follow)
  return resolve_err if resolve_err
  stat = symlink_follow ? File.stat(host_path) : File.lstat(host_path)
  a, m = resolve_times(stat, atim, mtim, fstflags)
  if symlink_follow
    File.utime(a, m, host_path)
  else
    File.lutime(a, m, host_path) # act on the symlink itself
  end
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

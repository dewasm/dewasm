# requires: memory/read_string, memory/init, wasi/resolve_path, wasi/pack_filestat, wasi/errno_fs
def wasi_path_filestat_get(dirfd, flags, path_ptr, path_len, buf_ptr)
  rel = @memory.read_string(path_ptr, path_len)
  symlink_follow = flags & 0x1 != 0 # lookupflags::SYMLINK_FOLLOW
  host_path, err = resolve_path(dirfd, rel, follow_last: symlink_follow)
  return err if err
  stat = symlink_follow ? File.stat(host_path) : File.lstat(host_path)
  @memory.init(buf_ptr, pack_filestat(stat), 0, 64)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

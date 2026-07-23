# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_rename(old_dirfd, old_path_ptr, old_path_len, new_dirfd, new_path_ptr, new_path_len)
  old_rel = @memory.read_string(old_path_ptr, old_path_len)
  old_host, err = resolve_path(old_dirfd, old_rel)
  return err if err
  new_rel = @memory.read_string(new_path_ptr, new_path_len)
  new_host, err = resolve_path(new_dirfd, new_rel)
  return err if err
  File.rename(old_host, new_host)
  ERRNO_SUCCESS
rescue Errno::ENOENT
  ERRNO_NOENT
rescue Errno::ENOTDIR
  ERRNO_NOTDIR
rescue Errno::ENOTEMPTY
  ERRNO_NOTEMPTY
rescue SystemCallError
  ERRNO_IO
end

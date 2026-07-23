# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_rename(old_dirfd, old_path_ptr, old_path_len, new_dirfd, new_path_ptr, new_path_len)
  old_rel = @memory.read_string(old_path_ptr, old_path_len)
  # rename(2) never follows trailing symlinks: it moves the link itself
  # and replaces the destination link.
  old_host, err = resolve_path(old_dirfd, old_rel, follow_last: false)
  return err if err
  new_rel = @memory.read_string(new_path_ptr, new_path_len)
  new_host, err = resolve_path(new_dirfd, new_rel, follow_last: false)
  return err if err
  File.rename(old_host, new_host)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

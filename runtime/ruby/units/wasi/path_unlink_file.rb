# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_unlink_file(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # unlink(2) never follows a trailing symlink: it removes the link.
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  File.unlink(host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

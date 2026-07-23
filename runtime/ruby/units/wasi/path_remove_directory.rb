# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_remove_directory(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # rmdir(2) never follows a trailing symlink.
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  Dir.rmdir(host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

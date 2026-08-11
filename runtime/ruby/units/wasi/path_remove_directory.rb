# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_remove_directory(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # rmdir(2) never follows a trailing symlink.
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  # rmdir through a trailing slash on an existing directory is EINVAL per
  # wasmtime; other shapes come from the host call.
  if host_path.end_with?("/") && File.directory?(host_path.delete_suffix("/"))
    return ERRNO_INVAL
  end
  Dir.rmdir(host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_remove_directory(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # rmdir(2) never follows a trailing symlink.
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  # wasmtime 47 (ADR-49) rejects removing an existing directory through a
  # slash-suffixed name with EINVAL on both hosts (cap-std's final-component
  # handling); a missing target stays ENOENT and a non-directory ENOTDIR via
  # the host call on the slash-preserved path.
  if host_path.end_with?("/") && File.directory?(host_path.delete_suffix("/"))
    return ERRNO_INVAL
  end
  Dir.rmdir(host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

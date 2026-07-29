# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_create_directory(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # Strip a trailing slash: mkdir names a directory anyway, and EEXIST is
  # wasmtime's answer for mkdir("file/") where the hosts split (ADR-49).
  rel = rel.sub(%r{(.)/+\z}, '\1')
  # mkdir(2) never follows a trailing symlink (an existing one is EEXIST).
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  Dir.mkdir(host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

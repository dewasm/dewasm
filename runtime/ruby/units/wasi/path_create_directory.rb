# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_create_directory(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # mkdir names a directory by definition, so a trailing slash adds nothing
  # but host-divergent errnos (macOS mkdir(2) reports ENOTDIR for "file/",
  # Linux EEXIST): strip it so the existing-target case is EEXIST uniformly
  # and "sub/" still creates (issue #42).
  rel = rel.sub(%r{(.)/+\z}, '\1')
  # mkdir(2) never follows a trailing symlink (an existing one is EEXIST).
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  Dir.mkdir(host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

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
  # The preserved slash lets the host rename(2) enforce the existing and
  # missing shapes; a *nonexistent* slash-suffixed destination is stripped
  # so the rename proceeds, as wasmtime does (issue #42). Probe the
  # bare path — stat on "x/" fails ENOTDIR and reads as missing.
  if new_host.end_with?("/")
    new_bare = new_host.delete_suffix("/")
    new_host = new_bare unless File.exist?(new_bare) || File.symlink?(new_bare)
  end
  File.rename(old_host, new_host)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

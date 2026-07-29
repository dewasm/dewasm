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
  # Trailing-slash semantics (issue #42, ADR-49: follow wasmtime): the
  # slash resolve_path preserved lets the host rename(2) enforce the
  # shapes wasmtime inherits from the OS — an existing non-directory
  # behind a slash is ENOTDIR on either side, a missing slash-suffixed
  # source is ENOENT. A *nonexistent* slash-suffixed destination is the
  # one shape wasmtime does not inherit: cap-std strips the slash and the
  # rename proceeds (even from a non-directory source), so strip it here
  # too. The existence probe uses the slash-stripped path: stat on the
  # slash-bearing one already fails ENOTDIR and would misread "exists as
  # a file" as "missing".
  if new_host.end_with?("/")
    new_bare = new_host.delete_suffix("/")
    new_host = new_bare unless File.exist?(new_bare) || File.symlink?(new_bare)
  end
  File.rename(old_host, new_host)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

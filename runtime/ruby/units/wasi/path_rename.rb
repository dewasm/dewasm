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
  # Trailing-slash semantics (issue #42): resolve_path preserved the slash,
  # so the host rename(2) enforces the uniform shapes itself — an existing
  # non-directory behind a slash is ENOTDIR on either side, a missing
  # slash-bearing source is ENOENT. The one shape Linux and macOS disagree
  # on — a nonexistent slash-bearing destination with a non-directory
  # source (macOS/POSIX ENOENT, Linux ENOTDIR) — is normalized to ENOENT
  # here, mirroring runtime/bash/units/wasi/path_rename.sh. The probes use
  # the slash-stripped path: stat on the slash-bearing one already fails
  # ENOTDIR and would misread "exists as a file" as "missing".
  if new_host.end_with?("/")
    old_bare = old_host.delete_suffix("/")
    new_bare = new_host.delete_suffix("/")
    new_exists = File.exist?(new_bare) || File.symlink?(new_bare)
    # When the source itself bears a slash over an existing non-directory,
    # fall through: the host reports the source-side ENOTDIR first.
    src_slash_ok = !old_host.end_with?("/") || File.directory?(old_bare)
    return ERRNO_NOENT if !new_exists && src_slash_ok && !File.directory?(old_bare)
  end
  File.rename(old_host, new_host)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

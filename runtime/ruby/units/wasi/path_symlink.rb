# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_symlink(old_ptr, old_len, dirfd, new_ptr, new_len)
  target = @memory.read_string(old_ptr, old_len)
  new_rel = @memory.read_string(new_ptr, new_len)
  # The link's *contents* are stored verbatim (containment is enforced when
  # a later resolve follows the link) — except an absolute target,
  # which could never resolve inside the sandbox, so reject it up front.
  return ERRNO_NOTCAPABLE if target.start_with?("/")
  host_path, err = resolve_path(dirfd, new_rel, follow_last: false)
  return err if err
  # A slash-suffixed link name needs an existing directory there. Probe the
  # slash-stripped path (stat on "file/" fails ENOTDIR, reads as missing).
  if new_rel.end_with?("/")
    bare = host_path.delete_suffix("/")
    if File.symlink?(bare) || File.exist?(bare)
      return File.directory?(bare) ? ERRNO_EXIST : ERRNO_NOTDIR
    end
    return ERRNO_NOENT
  end
  File.symlink(target, host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_symlink(old_ptr, old_len, dirfd, new_ptr, new_len)
  target = @memory.read_string(old_ptr, old_len)
  new_rel = @memory.read_string(new_ptr, new_len)
  # The link's *contents* are stored verbatim (containment is enforced when
  # a later resolve follows the link, ADR-40) — except an absolute target,
  # which could never resolve inside the sandbox, so reject it up front.
  return ERRNO_NOTCAPABLE if target.start_with?("/")
  host_path, err = resolve_path(dirfd, new_rel, follow_last: false)
  return err if err
  # A trailing slash on the link name demands an existing directory there;
  # File.symlink would otherwise create a slash-suffixed name.
  if new_rel.end_with?("/")
    if File.symlink?(host_path) || File.exist?(host_path)
      return File.directory?(host_path) ? ERRNO_EXIST : ERRNO_NOTDIR
    end
    return ERRNO_NOENT
  end
  File.symlink(target, host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

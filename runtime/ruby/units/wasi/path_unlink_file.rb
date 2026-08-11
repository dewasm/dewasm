# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_unlink_file(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # unlink(2) never follows a trailing symlink: it removes the link.
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  # A slash-suffixed non-directory is ENOTDIR, missing is ENOENT; a real
  # directory falls through to File.unlink (host EPERM/EISDIR). Probe the
  # slash-stripped path: stat on "file/" fails ENOTDIR, reads as missing.
  if host_path.end_with?("/")
    bare = host_path.delete_suffix("/")
    unless File.directory?(bare)
      return File.exist?(bare) || File.symlink?(bare) ? ERRNO_NOTDIR : ERRNO_NOENT
    end
  end
  File.unlink(host_path)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

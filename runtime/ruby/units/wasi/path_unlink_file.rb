# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_unlink_file(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  # unlink(2) never follows a trailing symlink: it removes the link.
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  # A trailing slash requires a directory; unlink of a non-directory then
  # fails ENOTDIR. (A real directory still falls through to File.unlink,
  # which raises EPERM/EISDIR as a directory should.) The existence probes
  # use the slash-stripped path — resolve_path preserves the slash (issue
  # #42) and stat on "file/" already fails ENOTDIR, which would misread
  # "exists as a file" as "missing".
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

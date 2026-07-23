# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_remove_directory(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  host_path, err = resolve_path(dirfd, rel)
  return err if err
  Dir.rmdir(host_path)
  ERRNO_SUCCESS
rescue Errno::ENOTEMPTY
  ERRNO_NOTEMPTY
rescue Errno::ENOENT
  ERRNO_NOENT
rescue Errno::ENOTDIR
  ERRNO_NOTDIR
rescue SystemCallError
  ERRNO_IO
end

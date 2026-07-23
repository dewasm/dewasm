# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_unlink_file(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  host_path, err = resolve_path(dirfd, rel)
  return err if err
  File.unlink(host_path)
  ERRNO_SUCCESS
rescue Errno::EISDIR
  ERRNO_ISDIR
rescue Errno::ENOENT
  ERRNO_NOENT
rescue SystemCallError
  ERRNO_IO
end

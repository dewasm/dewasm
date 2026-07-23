# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_create_directory(dirfd, path_ptr, path_len)
  rel = @memory.read_string(path_ptr, path_len)
  host_path, err = resolve_path(dirfd, rel)
  return err if err
  Dir.mkdir(host_path)
  ERRNO_SUCCESS
rescue Errno::EEXIST
  ERRNO_EXIST
rescue Errno::ENOENT
  ERRNO_NOENT
rescue Errno::ENOTDIR
  ERRNO_NOTDIR
rescue SystemCallError
  ERRNO_IO
end

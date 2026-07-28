# requires: memory/read_string, memory/init, memory/i32_store, wasi/resolve_path, wasi/errno_fs
def wasi_path_readlink(dirfd, path_ptr, path_len, buf_ptr, buf_len, bufused_ptr)
  rel = @memory.read_string(path_ptr, path_len)
  # readlink operates on the link itself: resolve the parent but not the
  # final component.
  host_path, err = resolve_path(dirfd, rel, follow_last: false)
  return err if err
  bytes = File.readlink(host_path).b # raises EINVAL when not a symlink
  n = [bytes.bytesize, buf_len].min
  @memory.init(buf_ptr, bytes, 0, n)
  @memory.i32_store(bufused_ptr, n)
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

# requires: memory/read_string, memory/init, memory/i32_store, wasi/resolve_path, wasi/errno_fs
def wasi_path_readlink(self, fd, path_ptr, path_len, buf_ptr, buf_len, bufused_ptr):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    host, err = self.resolve_path(fd, rel, False)
    if err is not None:
        return err
    try:
        target = os.readlink(host)
    except OSError as e:
        return self.fs_errno(e)
    data = target.encode("utf-8", "surrogateescape")
    # The content is truncated to buf_len (no NUL terminator); bufused reports
    # how many bytes were written.
    n = min(len(data), buf_len)
    self.memory.init(buf_ptr, data, 0, n)
    self.memory.i32_store(bufused_ptr, n)
    return self.ERRNO_SUCCESS

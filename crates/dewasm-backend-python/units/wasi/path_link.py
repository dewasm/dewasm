# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_link(self, old_fd, old_flags, old_path_ptr, old_path_len, new_fd, new_path_ptr, new_path_len):
    # path_link operates on the source link itself; SYMLINK_FOLLOW is rejected rather than silently dereferenced.
    if old_flags & 0x1:
        return self.ERRNO_INVAL
    old_rel = self.memory.read_string(old_path_ptr, old_path_len).decode("utf-8", "surrogateescape")
    old_host, err = self.resolve_path(old_fd, old_rel, False)
    if err is not None:
        return err
    new_rel = self.memory.read_string(new_path_ptr, new_path_len).decode("utf-8", "surrogateescape")
    new_host, err = self.resolve_path(new_fd, new_rel, False)
    if err is not None:
        return err
    try:
        try:
            os.link(old_host, new_host, follow_symlinks=False)
        except (NotImplementedError, ValueError):
            # Platform without linkat NOFOLLOW: the source is contained already.
            os.link(old_host, new_host)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS

# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_unlink_file(self, dirfd, path_ptr, path_len):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    # unlink(2) never follows a trailing symlink: it removes the link.
    host_path, err = self.resolve_path(dirfd, rel, False)
    if err is not None:
        return err
    try:
        os.unlink(host_path)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS

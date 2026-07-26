# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_remove_directory(self, dirfd, path_ptr, path_len):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    # rmdir(2) never follows a trailing symlink.
    host_path, err = self.resolve_path(dirfd, rel, False)
    if err is not None:
        return err
    try:
        os.rmdir(host_path)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS

# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_create_directory(self, dirfd, path_ptr, path_len):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    # mkdir names a directory by definition, so a trailing slash adds nothing
    # but host-divergent errnos (macOS mkdir(2) reports ENOTDIR for "file/",
    # Linux EEXIST): strip it so the existing-target case is EEXIST uniformly
    # and "sub/" still creates (issue #42).
    core = rel.rstrip("/")
    if core:
        rel = core
    # mkdir(2) never follows a trailing symlink (an existing one is EEXIST).
    host_path, err = self.resolve_path(dirfd, rel, False)
    if err is not None:
        return err
    try:
        os.mkdir(host_path)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS

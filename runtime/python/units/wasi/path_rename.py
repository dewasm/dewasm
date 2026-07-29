# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_rename(self, old_dirfd, old_path_ptr, old_path_len, new_dirfd, new_path_ptr, new_path_len):
    old_rel = self.memory.read_string(old_path_ptr, old_path_len).decode("utf-8", "surrogateescape")
    # rename(2) never follows trailing symlinks: it moves the link itself and
    # replaces the destination link.
    old_host, err = self.resolve_path(old_dirfd, old_rel, False)
    if err is not None:
        return err
    new_rel = self.memory.read_string(new_path_ptr, new_path_len).decode("utf-8", "surrogateescape")
    new_host, err = self.resolve_path(new_dirfd, new_rel, False)
    if err is not None:
        return err
    # The preserved slash lets the host rename(2) enforce the existing and
    # missing shapes; a *nonexistent* slash-suffixed destination is stripped
    # so the rename proceeds, as wasmtime does (issue #42, ADR-49). Probe the
    # bare path — stat on "x/" fails ENOTDIR and reads as missing.
    if new_host.endswith(os.sep) and not os.path.lexists(new_host[:-1]):
        new_host = new_host[:-1]
    try:
        os.rename(old_host, new_host)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS
